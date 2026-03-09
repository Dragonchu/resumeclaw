use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::llm::provider::ToolDefinition;
use crate::mailer::{default_resume_attachment, send_email, EmailRequest};

use super::{ToolHandler, ToolResult};

pub struct SendResumeEmail {
    workspace: PathBuf,
}

impl SendResumeEmail {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }
}

#[async_trait]
impl ToolHandler for SendResumeEmail {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "send_resume_email".to_string(),
            description: "Send the current compiled resume PDF to a specified email address. Requires the target email, subject, and plain-text body. The recipient must be listed in SMTP_ALLOWED_RECIPIENTS. Call compile_resume before this tool if the PDF is not up to date.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Recipient email address"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Email subject line"
                    },
                    "body": {
                        "type": "string",
                        "description": "Plain-text email body"
                    }
                },
                "required": ["to", "subject", "body"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let to = match required_string(&args, "to") {
            Ok(value) => value,
            Err(text) => {
                return ToolResult {
                    text,
                    attachments: vec![],
                }
            }
        };
        let subject = match required_string(&args, "subject") {
            Ok(value) => value,
            Err(text) => {
                return ToolResult {
                    text,
                    attachments: vec![],
                }
            }
        };
        let body = match required_string(&args, "body") {
            Ok(value) => value,
            Err(text) => {
                return ToolResult {
                    text,
                    attachments: vec![],
                }
            }
        };

        let attachment_path = default_resume_attachment(&self.workspace);
        if !attachment_path.exists() {
            return ToolResult {
                text: format!(
                    "Error: resume attachment not found at {}. Please call compile_resume first.",
                    attachment_path.display()
                ),
                attachments: vec![],
            };
        }

        let request = EmailRequest {
            to,
            subject,
            body,
            attachment_path,
        };

        match send_email(request).await {
            Ok(()) => ToolResult {
                text: "Email sent successfully with the resume attachment.".to_string(),
                attachments: vec![],
            },
            Err(err) => ToolResult {
                text: format!("Failed to send resume email: {err}"),
                attachments: vec![],
            },
        }
    }
}

fn required_string(args: &serde_json::Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("Error: missing '{key}' parameter"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::SendResumeEmail;
    use crate::tools::ToolHandler;

    fn temp_workspace() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("resumeclaw-email-tool-{unique}"));
        std::fs::create_dir_all(&path).expect("temp workspace should be created");
        path
    }

    #[tokio::test]
    async fn rejects_missing_subject() {
        let workspace = temp_workspace();
        let tool = SendResumeEmail::new(&workspace);

        let result = tool
            .execute(json!({"to": "user@example.com", "body": "hello"}))
            .await;

        assert_eq!(result.text, "Error: missing 'subject' parameter");
        std::fs::remove_dir_all(workspace).expect("temp workspace should be removed");
    }

    #[tokio::test]
    async fn requires_compiled_resume_pdf() {
        let workspace = temp_workspace();
        let tool = SendResumeEmail::new(&workspace);

        let result = tool
            .execute(json!({
                "to": "user@example.com",
                "subject": "简历",
                "body": "附件是我的简历"
            }))
            .await;

        assert!(result.text.contains("Please call compile_resume first"));
        std::fs::remove_dir_all(workspace).expect("temp workspace should be removed");
    }
}
