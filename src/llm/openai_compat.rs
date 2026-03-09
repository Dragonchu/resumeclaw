//! OpenAI Chat Completions compatible provider.
//!
//! Works with: OpenAI, DeepSeek, Ollama, Groq, Together, xAI, etc.
//! Any endpoint that speaks the OpenAI Chat Completions API.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::provider::{
    ChatMessage, CompletionResponse, LlmError, LlmProvider, Role, ToolCall, ToolDefinition,
};

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: crate::proxy::build_client().expect("failed to build HTTP client"),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    fn build_messages(&self, messages: &[ChatMessage]) -> Vec<ApiMessage> {
        messages.iter().map(|m| {
            let api_tool_calls = if m.tool_calls.is_empty() {
                None
            } else {
                Some(m.tool_calls.iter().map(|tc| ApiToolCall {
                    id: tc.id.clone(),
                    tool_type: "function".to_string(),
                    function: ApiFunction {
                        name: tc.name.clone(),
                        arguments: tc.arguments.to_string(),
                    },
                }).collect())
            };

            // Assistant messages with only tool_calls should have content: null
            let content = if m.content.is_empty() && m.role == Role::Assistant {
                None
            } else {
                Some(m.content.clone())
            };

            ApiMessage {
                role: m.role,
                content,
                tool_call_id: m.tool_call_id.clone(),
                tool_calls: api_tool_calls,
            }
        }).collect()
    }

    async fn send_request(&self, body: serde_json::Value) -> Result<ApiResponse, LlmError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed { reason: format!("{e:?}") })?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LlmError::AuthFailed { provider: self.model.clone() });
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::RequestFailed {
                reason: format!("HTTP {status}: {text}"),
            });
        }

        resp.json::<ApiResponse>()
            .await
            .map_err(|e| LlmError::RequestFailed { reason: e.to_string() })
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, messages: Vec<ChatMessage>) -> Result<String, LlmError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": self.build_messages(&messages),
        });

        let resp = self.send_request(body).await?;
        Ok(extract_text(&resp))
    }

    async fn complete_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<CompletionResponse, LlmError> {
        let api_tools: Vec<serde_json::Value> = tools.iter().map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        }).collect();

        let body = serde_json::json!({
            "model": self.model,
            "messages": self.build_messages(&messages),
            "tools": api_tools,
        });

        let resp = self.send_request(body).await?;
        Ok(extract_completion(&resp))
    }
}

// --- OpenAI API types (minimal) ---

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ApiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    choices: Vec<ApiChoice>,
}

#[derive(Debug, Deserialize)]
struct ApiChoice {
    message: ApiChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ApiChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ApiToolCall>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiToolCall {
    id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    tool_type: String,
    function: ApiFunction,
}

fn default_tool_type() -> String {
    "function".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiFunction {
    name: String,
    arguments: String, // JSON string
}

fn extract_text(resp: &ApiResponse) -> String {
    resp.choices.first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default()
}

fn extract_completion(resp: &ApiResponse) -> CompletionResponse {
    let choice = match resp.choices.first() {
        Some(c) => c,
        None => return CompletionResponse { content: None, tool_calls: vec![] },
    };

    let tool_calls: Vec<ToolCall> = choice.message.tool_calls.iter().map(|tc| {
        let args = serde_json::from_str(&tc.function.arguments)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        ToolCall {
            id: tc.id.clone(),
            name: tc.function.name.clone(),
            arguments: args,
        }
    }).collect();

    CompletionResponse {
        content: choice.message.content.clone(),
        tool_calls,
    }
}
