use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::Deserialize;

use super::provider::{
    ChatMessage, CompletionResponse, LlmError, LlmProvider, Role, ToolCall, ToolDefinition,
};

const DEV_REPL_FALLBACK_PREFIX: &str = "开发模式仍在使用内置 Mock Provider。\n已收到你的消息：";
const DEV_REPL_FALLBACK_SUFFIX: &str =
    "\n这是零配置调试回显；如需真实多轮对话，请配置 LLM_PROVIDER。";

#[derive(Debug, Deserialize)]
struct MockCompletionStep {
    #[serde(default)]
    expect_last_user_message: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

/// A deterministic LLM provider backed by a local JSON script.
///
/// This is intended for local integration tests and manual smoke testing
/// without calling a real model API.
pub struct MockProvider {
    model: String,
    steps: Mutex<VecDeque<MockCompletionStep>>,
    repeat_on_exhaustion: bool,
}

impl MockProvider {
    pub fn from_env(model: &str) -> Result<Self, LlmError> {
        let path = std::env::var("MOCK_LLM_SCRIPT_PATH").map_err(|_| LlmError::AuthFailed {
            provider: "mock: MOCK_LLM_SCRIPT_PATH not set".to_string(),
        })?;
        Self::from_path_with_options(&path, model, repeat_on_exhaustion_from_env())
    }

    #[allow(dead_code)]
    pub fn from_path(path: impl AsRef<Path>, model: &str) -> Result<Self, LlmError> {
        Self::from_path_with_options(path, model, false)
    }

    /// Load a mock script from disk, optionally enabling a REPL-style fallback
    /// that keeps returning echo responses after the scripted steps are used up.
    fn from_path_with_options(
        path: impl AsRef<Path>,
        model: &str,
        repeat_on_exhaustion: bool,
    ) -> Result<Self, LlmError> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(|e| LlmError::RequestFailed {
            reason: format!("failed to read mock script {}: {e}", path.display()),
        })?;
        let steps: Vec<MockCompletionStep> =
            serde_json::from_str(&raw).map_err(|e| LlmError::RequestFailed {
                reason: format!("failed to parse mock script {}: {e}", path.display()),
            })?;

        if steps.is_empty() {
            return Err(LlmError::RequestFailed {
                reason: format!("mock script {} is empty", path.display()),
            });
        }

        Ok(Self {
            model: model.to_string(),
            steps: Mutex::new(VecDeque::from(steps)),
            repeat_on_exhaustion,
        })
    }

    fn next_step(&self, messages: &[ChatMessage]) -> Result<CompletionResponse, LlmError> {
        let mut steps = self.steps.lock().map_err(|_| LlmError::RequestFailed {
            reason: "internal error: mock provider lock poisoned; a previous mock LLM call likely panicked".to_string(),
        })?;
        let step = match steps.pop_front() {
            Some(step) => step,
            None if self.repeat_on_exhaustion => return Ok(repl_fallback_response(messages)),
            None => {
                return Err(LlmError::RequestFailed {
                    reason: "mock script exhausted before conversation completed".to_string(),
                });
            }
        };
        if let Some(expected) = step.expect_last_user_message.as_deref() {
            let actual = messages
                .iter()
                .rev()
                .find(|message| message.role == Role::User)
                .map(|message| message.content.as_str());
            if actual != Some(expected) {
                return Err(LlmError::RequestFailed {
                    reason: format!(
                        "mock script expected last user message {:?}, got {:?}",
                        expected, actual
                    ),
                });
            }
        }
        Ok(CompletionResponse {
            content: step.content,
            tool_calls: step.tool_calls,
        })
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, messages: Vec<ChatMessage>) -> Result<String, LlmError> {
        let step = self.next_step(&messages)?;
        Ok(step.content.unwrap_or_default())
    }

    async fn complete_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Vec<ToolDefinition>,
    ) -> Result<CompletionResponse, LlmError> {
        self.next_step(&messages)
    }
}

/// Read `MOCK_LLM_REPEAT_ON_EXHAUSTION` and enable repeat mode when it is set
/// to `1`, `true`, or `yes` in any letter case.
fn repeat_on_exhaustion_from_env() -> bool {
    std::env::var("MOCK_LLM_REPEAT_ON_EXHAUSTION")
        .ok()
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

/// Build the REPL-style echo response used after the scripted mock conversation
/// is exhausted while repeat-on-exhaustion mode is enabled.
fn repl_fallback_response(messages: &[ChatMessage]) -> CompletionResponse {
    let last_user_message = messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .map(|message| message.content.trim())
        .filter(|message| !message.is_empty())
        .unwrap_or("空消息");

    CompletionResponse {
        content: Some(format!(
            "{DEV_REPL_FALLBACK_PREFIX}{last_user_message}{DEV_REPL_FALLBACK_SUFFIX}"
        )),
        tool_calls: vec![],
    }
}
