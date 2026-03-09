//! Resume editing and compilation tools.

use std::ffi::OsString;
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
        Self { workspace: workspace.to_path_buf() }
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
            Ok(content) => ToolResult { text: content, attachments: vec![] },
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
        Self { workspace: workspace.to_path_buf() }
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
            None => return ToolResult {
                text: "Error: missing 'content' parameter".to_string(),
                attachments: vec![],
            },
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
        Self { workspace: workspace.to_path_buf() }
    }
}

#[async_trait]
impl ToolHandler for CompileResume {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "compile_resume".to_string(),
            description: "Compile the resume LaTeX file to PDF using tectonic. Returns the compilation result. On success, the PDF will be automatically sent to the user.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _args: serde_json::Value) -> ToolResult {
        let output = tokio::process::Command::new(tectonic_bin())
            .arg("resume.tex")
            .current_dir(&self.workspace)
            .output()
            .await;

        let output = match output {
            Ok(o) => o,
            Err(e) => return ToolResult {
                text: format!("Failed to run tectonic: {e}. Is tectonic installed?"),
                attachments: vec![],
            },
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
                    text: "tectonic exited successfully but resume.pdf was not found.".to_string(),
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
                text: format!("Compilation failed.\n\nstderr:\n{stderr}\n\nlog (last 50 lines):\n{log}"),
                attachments: vec![],
            }
        }
    }
}

/// Resolve the tectonic executable path, allowing tests to override it.
fn tectonic_bin() -> OsString {
    std::env::var_os("TECTONIC_BIN").unwrap_or_else(|| OsString::from("tectonic"))
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::tools::ToolHandler;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("resumeclaw-{name}-{nanos}"))
    }

    #[tokio::test]
    async fn compile_resume_uses_tectonic_and_returns_pdf_attachment() {
        let _env_lock = ENV_LOCK.lock().expect("lock env");

        let root = unique_test_dir("tectonic-compile");
        fs::create_dir_all(&root).expect("create root");
        fs::write(
            root.join("resume.tex"),
            r"\documentclass{article}
\begin{document}
Test
\end{document}
",
        )
        .expect("write resume.tex");

        let compiler_path = root.join("fake-tectonic.sh");
        fs::write(
            &compiler_path,
            r#"#!/bin/sh
printf '%s\n' "$@" > tectonic.args
> resume.pdf
"#,
        )
        .expect("write fake tectonic");
        let mut permissions = fs::metadata(&compiler_path)
            .expect("stat fake tectonic")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&compiler_path, permissions).expect("chmod fake tectonic");

        let old_tectonic_bin = std::env::var_os("TECTONIC_BIN");
        std::env::set_var("TECTONIC_BIN", &compiler_path);

        let result = CompileResume::new(&root).execute(serde_json::json!({})).await;

        if let Some(old_tectonic_bin) = old_tectonic_bin {
            std::env::set_var("TECTONIC_BIN", old_tectonic_bin);
        } else {
            std::env::remove_var("TECTONIC_BIN");
        }

        assert_eq!(result.text, "Compilation successful. PDF generated.");
        assert_eq!(result.attachments, vec![root.join("resume.pdf")]);

        let args = fs::read_to_string(root.join("tectonic.args")).expect("read tectonic args");
        assert_eq!(args, "resume.tex\n");

        fs::remove_dir_all(&root).expect("remove temp dir");
    }
}
