//! Anthropic Messages API provider.
//!
//! Speaks the native Anthropic protocol (not OpenAI-compatible).
//! https://docs.anthropic.com/en/api/messages

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::provider::{
    ChatMessage, CompletionResponse, LlmError, LlmProvider, Role, ToolCall, ToolDefinition,
};

pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: crate::proxy::build_client().expect("failed to build HTTP client"),
            base_url: "https://api.anthropic.com".to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    /// Anthropic uses a separate `system` field, not a system message in the array.
    fn split_messages(&self, messages: &[ChatMessage]) -> (Option<String>, Vec<AnthropicMessage>) {
        let system: Vec<&str> = messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.as_str())
            .collect();

        let system_prompt = if system.is_empty() {
            None
        } else {
            Some(system.join("\n"))
        };

        let msgs: Vec<AnthropicMessage> = messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                let role = match m.role {
                    Role::User | Role::Tool => "user",
                    Role::Assistant => "assistant",
                    Role::System => unreachable!(),
                };

                // Tool results use structured content blocks.
                if m.role == Role::Tool {
                    if let Some(ref tool_call_id) = m.tool_call_id {
                        return AnthropicMessage {
                            role: role.to_string(),
                            content: AnthropicContent::Blocks(vec![ContentBlock::ToolResult {
                                tool_use_id: tool_call_id.clone(),
                                content: m.content.clone(),
                            }]),
                        };
                    }
                }

                AnthropicMessage {
                    role: role.to_string(),
                    content: AnthropicContent::Text(m.content.clone()),
                }
            })
            .collect();

        (system_prompt, msgs)
    }

    async fn send_request(&self, body: serde_json::Value) -> Result<AnthropicResponse, LlmError> {
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed {
                reason: e.to_string(),
            })?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LlmError::AuthFailed {
                provider: "anthropic".to_string(),
            });
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::RequestFailed {
                reason: format!("HTTP {status}: {text}"),
            });
        }

        resp.json::<AnthropicResponse>()
            .await
            .map_err(|e| LlmError::RequestFailed {
                reason: e.to_string(),
            })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, messages: Vec<ChatMessage>) -> Result<String, LlmError> {
        let (system, msgs) = self.split_messages(&messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": msgs,
        });
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys);
        }

        let resp = self.send_request(body).await?;
        Ok(extract_text(&resp))
    }

    async fn complete_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<CompletionResponse, LlmError> {
        let (system, msgs) = self.split_messages(&messages);

        let api_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": msgs,
            "tools": api_tools,
        });
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys);
        }

        let resp = self.send_request(body).await?;
        Ok(extract_completion(&resp))
    }
}

// --- Anthropic API types (minimal) ---

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ResponseBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponseBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

fn extract_text(resp: &AnthropicResponse) -> String {
    resp.content
        .iter()
        .filter_map(|b| match b {
            ResponseBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn extract_completion(resp: &AnthropicResponse) -> CompletionResponse {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &resp.content {
        match block {
            ResponseBlock::Text { text } => text_parts.push(text.as_str()),
            ResponseBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: input.clone(),
                });
            }
        }
    }

    CompletionResponse {
        content: if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        },
        tool_calls,
    }
}
