//! Resume editing and compilation tools.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::llm::provider::ToolDefinition;

use super::{ToolHandler, ToolResult};

// ---------------------------------------------------------------------------
// read_resume
// ---------------------------------------------------------------------------

pub struct ReadResume {
    workspace: PathBuf,
}

impl ReadResume {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }
}

#[async_trait]
impl ToolHandler for ReadResume {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_resume".to_string(),
            description: "Read the current resume LaTeX source file (.tex). Call this before making any changes.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _args: serde_json::Value) -> ToolResult {
        let path = self.workspace.join("resume.tex");
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => ToolResult {
                text: content,
                attachments: vec![],
            },
            Err(e) => ToolResult {
                text: format!("Error reading resume.tex: {e}"),
                attachments: vec![],
            },
        }
    }
}

// ---------------------------------------------------------------------------
// write_resume
// ---------------------------------------------------------------------------

pub struct WriteResume {
    workspace: PathBuf,
}

impl WriteResume {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }
}

#[async_trait]
impl ToolHandler for WriteResume {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_resume".to_string(),
            description: "Write the complete LaTeX content to the resume file. You must provide the full .tex file content including \\documentclass, \\begin{document}, etc.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The complete LaTeX content for the resume .tex file"
                    }
                },
                "required": ["content"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return ToolResult {
                    text: "Error: missing 'content' parameter".to_string(),
                    attachments: vec![],
                }
            }
        };

        let path = self.workspace.join("resume.tex");
        match tokio::fs::write(&path, content).await {
            Ok(_) => ToolResult {
                text: format!("Successfully wrote {} bytes to resume.tex", content.len()),
                attachments: vec![],
            },
            Err(e) => ToolResult {
                text: format!("Error writing resume.tex: {e}"),
                attachments: vec![],
            },
        }
    }
}

// ---------------------------------------------------------------------------
// compile_resume
// ---------------------------------------------------------------------------

pub struct CompileResume {
    workspace: PathBuf,
}

impl CompileResume {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }
}

#[async_trait]
impl ToolHandler for CompileResume {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "compile_resume".to_string(),
            description: "Compile the resume LaTeX file to PDF using xelatex. Returns the compilation result. On success, the PDF will be automatically sent to the user.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _args: serde_json::Value) -> ToolResult {
        let output = tokio::process::Command::new("xelatex")
            .arg("-interaction=nonstopmode")
            .arg("-halt-on-error")
            .arg("resume.tex")
            .current_dir(&self.workspace)
            .output()
            .await;

        let output = match output {
            Ok(o) => o,
            Err(e) => {
                return ToolResult {
                    text: format!("Failed to run xelatex: {e}. Is xelatex installed?"),
                    attachments: vec![],
                }
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            let pdf_path = self.workspace.join("resume.pdf");
            if pdf_path.exists() {
                ToolResult {
                    text: "Compilation successful. PDF generated.".to_string(),
                    attachments: vec![pdf_path],
                }
            } else {
                ToolResult {
                    text: "xelatex exited successfully but resume.pdf was not found.".to_string(),
                    attachments: vec![],
                }
            }
        } else {
            // Return last 50 lines of output to help the LLM fix errors
            let log: String = stdout
                .lines()
                .rev()
                .take(50)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            ToolResult {
                text: format!(
                    "Compilation failed.\n\nstderr:\n{stderr}\n\nlog (last 50 lines):\n{log}"
                ),
                attachments: vec![],
            }
        }
    }
}
