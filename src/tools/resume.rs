//! Resume editing and compilation tools.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::llm::provider::ToolDefinition;

use super::{ToolHandler, ToolResult};

const VERSION_ROOT_DIR: &str = ".resumeclaw";
const VERSION_FILES_DIR: &str = "versions";
const VERSION_HEAD_FILE: &str = "HEAD";

#[derive(Clone)]
struct ResumeVersionStore {
    workspace: PathBuf,
}

impl ResumeVersionStore {
    fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }

    fn resume_path(&self) -> PathBuf {
        self.workspace.join("resume.tex")
    }

    fn version_root(&self) -> PathBuf {
        self.workspace.join(VERSION_ROOT_DIR)
    }

    fn versions_dir(&self) -> PathBuf {
        self.version_root().join(VERSION_FILES_DIR)
    }

    fn head_path(&self) -> PathBuf {
        self.version_root().join(VERSION_HEAD_FILE)
    }

    fn version_path(&self, version: u64) -> PathBuf {
        self.versions_dir().join(format!("{version:06}.tex"))
    }

    async fn ensure_initialized(&self) -> io::Result<()> {
        tokio::fs::create_dir_all(self.versions_dir()).await?;

        let versions = self.version_numbers().await?;
        if versions.is_empty() {
            match tokio::fs::read_to_string(self.resume_path()).await {
                Ok(content) => {
                    tokio::fs::write(self.version_path(1), content).await?;
                    self.write_head(1).await?;
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            return Ok(());
        }

        if self.read_head().await?.is_none() {
            if let Some(version) = versions.last().copied() {
                self.write_head(version).await?;
            }
        }

        Ok(())
    }

    async fn version_numbers(&self) -> io::Result<Vec<u64>> {
        let mut entries = match tokio::fs::read_dir(self.versions_dir()).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(err) => return Err(err),
        };

        let mut versions = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("tex") {
                continue;
            }

            if let Some(version) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u64>().ok())
            {
                versions.push(version);
            }
        }

        versions.sort_unstable();
        Ok(versions)
    }

    async fn read_head(&self) -> io::Result<Option<u64>> {
        match tokio::fs::read_to_string(self.head_path()).await {
            Ok(content) => Ok(content.trim().parse::<u64>().ok()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn write_head(&self, version: u64) -> io::Result<()> {
        tokio::fs::create_dir_all(self.version_root()).await?;
        tokio::fs::write(self.head_path(), version.to_string()).await
    }

    async fn read_version(&self, version: u64) -> io::Result<String> {
        tokio::fs::read_to_string(self.version_path(version)).await
    }

    async fn append_version(&self, content: &str) -> io::Result<u64> {
        self.ensure_initialized().await?;

        let next_version = self
            .version_numbers()
            .await?
            .last()
            .copied()
            .map(|current| current + 1)
            .unwrap_or(1);

        tokio::fs::write(self.version_path(next_version), content).await?;
        self.write_head(next_version).await?;
        tokio::fs::write(self.resume_path(), content).await?;
        Ok(next_version)
    }

    async fn head_and_versions(&self) -> io::Result<(Option<u64>, Vec<u64>)> {
        self.ensure_initialized().await?;
        let head = self.read_head().await?;
        let versions = self.version_numbers().await?;
        Ok((head, versions))
    }

    async fn redirect_head(
        &self,
        absolute_version: Option<u64>,
        offset: Option<i64>,
    ) -> Result<HeadRedirection, String> {
        let (head, versions) = self
            .head_and_versions()
            .await
            .map_err(|err| format!("Error loading version history: {err}"))?;
        let current_head = head.ok_or_else(|| "No resume versions available".to_string())?;

        let requested = match (absolute_version, offset) {
            (Some(_), Some(_)) => {
                return Err("Provide either 'version' or 'offset', not both".to_string())
            }
            (None, None) => return Err("Missing 'version' or 'offset' parameter".to_string()),
            (Some(version), None) => version,
            (None, Some(offset)) => {
                let current_index = versions
                    .iter()
                    .position(|version| *version == current_head)
                    .ok_or_else(|| {
                        format!("Current HEAD v{current_head} is missing from history")
                    })?;
                let target_index = current_index as i64 + offset;
                if target_index < 0 || target_index >= versions.len() as i64 {
                    return Err(format!(
                        "Offset {offset} moves outside version history (HEAD is v{current_head})"
                    ));
                }
                versions[target_index as usize]
            }
        };

        if !versions.contains(&requested) {
            return Err(format!("Version v{requested} does not exist"));
        }

        let content = self
            .read_version(requested)
            .await
            .map_err(|err| format!("Error reading version v{requested}: {err}"))?;
        tokio::fs::write(self.resume_path(), &content)
            .await
            .map_err(|err| format!("Error updating resume.tex: {err}"))?;
        self.write_head(requested)
            .await
            .map_err(|err| format!("Error updating HEAD pointer: {err}"))?;

        Ok(HeadRedirection {
            previous_head: current_head,
            head: requested,
            bytes: content.len(),
        })
    }
}

struct HeadRedirection {
    previous_head: u64,
    head: u64,
    bytes: usize,
}

// ---------------------------------------------------------------------------
// read_resume
// ---------------------------------------------------------------------------

pub struct ReadResume {
    store: ResumeVersionStore,
}

impl ReadResume {
    pub fn new(workspace: &Path) -> Self {
        Self {
            store: ResumeVersionStore::new(workspace),
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
        if let Err(err) = self.store.ensure_initialized().await {
            return ToolResult {
                text: format!("Error initializing resume version store: {err}"),
                attachments: vec![],
            };
        }

        let path = self.store.resume_path();
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
    store: ResumeVersionStore,
}

impl WriteResume {
    pub fn new(workspace: &Path) -> Self {
        Self {
            store: ResumeVersionStore::new(workspace),
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

        match self.store.append_version(content).await {
            Ok(version) => ToolResult {
                text: format!(
                    "Successfully wrote {} bytes to resume.tex as version v{version}",
                    content.len()
                ),
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
// list_versions
// ---------------------------------------------------------------------------

pub struct ListVersions {
    store: ResumeVersionStore,
}

impl ListVersions {
    pub fn new(workspace: &Path) -> Self {
        Self {
            store: ResumeVersionStore::new(workspace),
        }
    }
}

#[async_trait]
impl ToolHandler for ListVersions {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_versions".to_string(),
            description: "List all resume versions and identify the current HEAD version."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _args: serde_json::Value) -> ToolResult {
        let (head, versions) = match self.store.head_and_versions().await {
            Ok(state) => state,
            Err(err) => {
                return ToolResult {
                    text: format!("Error listing resume versions: {err}"),
                    attachments: vec![],
                }
            }
        };

        if versions.is_empty() {
            return ToolResult {
                text: "No resume versions available yet.".to_string(),
                attachments: vec![],
            };
        }

        let mut lines = vec![format!("Current HEAD: v{}", head.unwrap_or(versions[0]))];
        for version in versions {
            let bytes = match tokio::fs::metadata(self.store.version_path(version)).await {
                Ok(metadata) => metadata.len() as usize,
                Err(_) => 0,
            };
            let marker = if Some(version) == head { " [HEAD]" } else { "" };
            lines.push(format!("- v{version}{marker} ({bytes} bytes)"));
        }

        ToolResult {
            text: lines.join("\n"),
            attachments: vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// get_resume_by_version
// ---------------------------------------------------------------------------

pub struct GetResumeByVersion {
    store: ResumeVersionStore,
}

impl GetResumeByVersion {
    pub fn new(workspace: &Path) -> Self {
        Self {
            store: ResumeVersionStore::new(workspace),
        }
    }
}

#[async_trait]
impl ToolHandler for GetResumeByVersion {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_resume_by_version".to_string(),
            description: "Read the LaTeX content of a specific historical resume version."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "version": {
                        "type": "integer",
                        "description": "The resume version number to inspect"
                    }
                },
                "required": ["version"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let Some(version) = args.get("version").and_then(|value| value.as_u64()) else {
            return ToolResult {
                text: "Error: missing 'version' parameter".to_string(),
                attachments: vec![],
            };
        };

        let head = match self.store.head_and_versions().await {
            Ok((head, _)) => head,
            Err(err) => {
                return ToolResult {
                    text: format!("Error loading resume versions: {err}"),
                    attachments: vec![],
                }
            }
        };

        match self.store.read_version(version).await {
            Ok(content) => {
                let marker = if head == Some(version) { "yes" } else { "no" };
                ToolResult {
                    text: format!("Version: v{version}\nHEAD: {marker}\n\n{content}"),
                    attachments: vec![],
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => ToolResult {
                text: format!("Version v{version} does not exist"),
                attachments: vec![],
            },
            Err(err) => ToolResult {
                text: format!("Error reading version v{version}: {err}"),
                attachments: vec![],
            },
        }
    }
}

// ---------------------------------------------------------------------------
// redirect_resume_version
// ---------------------------------------------------------------------------

pub struct RedirectResumeVersion {
    store: ResumeVersionStore,
}

impl RedirectResumeVersion {
    pub fn new(workspace: &Path) -> Self {
        Self {
            store: ResumeVersionStore::new(workspace),
        }
    }
}

#[async_trait]
impl ToolHandler for RedirectResumeVersion {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "redirect_resume_version".to_string(),
            description: "Move the HEAD pointer to another resume version by absolute version number or relative offset, updating resume.tex to match.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "version": {
                        "type": "integer",
                        "description": "Absolute version number to point HEAD to"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Relative movement from the current HEAD. Negative rolls back, positive fast-forwards."
                    }
                }
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let version = args.get("version").and_then(|value| value.as_u64());
        let offset = args.get("offset").and_then(|value| value.as_i64());

        match self.store.redirect_head(version, offset).await {
            Ok(redirect) => ToolResult {
                text: format!(
                    "Moved HEAD from v{} to v{} and synced resume.tex ({} bytes)",
                    redirect.previous_head, redirect.head, redirect.bytes
                ),
                attachments: vec![],
            },
            Err(err) => ToolResult {
                text: format!("Error redirecting resume version: {err}"),
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
            Err(e) => {
                return ToolResult {
                    text: format!("Failed to run tectonic: {e}. Is tectonic installed?"),
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
                text: format!(
                    "Compilation failed.\n\nstderr:\n{stderr}\n\nlog (last 50 lines):\n{log}"
                ),
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

        let result = CompileResume::new(&root)
            .execute(serde_json::json!({}))
            .await;

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

    #[tokio::test]
    async fn version_tools_track_head_and_support_redirects() {
        let root = unique_test_dir("resume-versioning");
        fs::create_dir_all(&root).expect("create root");
        fs::write(
            root.join("resume.tex"),
            r"\documentclass{article}
\begin{document}
Version One
\end{document}
",
        )
        .expect("write initial resume");

        let write_resume = WriteResume::new(&root);
        let list_versions = ListVersions::new(&root);
        let get_resume_by_version = GetResumeByVersion::new(&root);
        let redirect_resume_version = RedirectResumeVersion::new(&root);

        let write_v2 = write_resume
            .execute(serde_json::json!({
                "content": "\\documentclass{article}\n\\begin{document}\nVersion Two\n\\end{document}\n"
            }))
            .await;
        assert!(
            write_v2.text.contains("version v2"),
            "unexpected write result: {}",
            write_v2.text
        );

        let write_v3 = write_resume
            .execute(serde_json::json!({
                "content": "\\documentclass{article}\n\\begin{document}\nVersion Three\n\\end{document}\n"
            }))
            .await;
        assert!(
            write_v3.text.contains("version v3"),
            "unexpected write result: {}",
            write_v3.text
        );

        let versions = list_versions.execute(serde_json::json!({})).await;
        assert!(
            versions.text.contains("Current HEAD: v3"),
            "missing head marker: {}",
            versions.text
        );
        assert!(
            versions.text.contains("- v1"),
            "missing v1 in list: {}",
            versions.text
        );
        assert!(
            versions.text.contains("- v2"),
            "missing v2 in list: {}",
            versions.text
        );
        assert!(
            versions.text.contains("- v3 [HEAD]"),
            "missing v3 head in list: {}",
            versions.text
        );

        let version_two = get_resume_by_version
            .execute(serde_json::json!({ "version": 2 }))
            .await;
        assert!(
            version_two.text.contains("Version: v2"),
            "missing version header: {}",
            version_two.text
        );
        assert!(
            version_two.text.contains("Version Two"),
            "missing historical content: {}",
            version_two.text
        );
        assert!(
            version_two.text.contains("HEAD: no"),
            "unexpected head marker: {}",
            version_two.text
        );

        let rollback = redirect_resume_version
            .execute(serde_json::json!({ "offset": -2 }))
            .await;
        assert!(
            rollback.text.contains("Moved HEAD from v3 to v1"),
            "rollback failed: {}",
            rollback.text
        );
        let rolled_back_resume =
            fs::read_to_string(root.join("resume.tex")).expect("read rolled back resume");
        assert!(
            rolled_back_resume.contains("Version One"),
            "resume.tex did not roll back:\n{rolled_back_resume}"
        );

        let fast_forward = redirect_resume_version
            .execute(serde_json::json!({ "version": 3 }))
            .await;
        assert!(
            fast_forward.text.contains("Moved HEAD from v1 to v3"),
            "fast-forward failed: {}",
            fast_forward.text
        );
        let forwarded_resume =
            fs::read_to_string(root.join("resume.tex")).expect("read fast-forwarded resume");
        assert!(
            forwarded_resume.contains("Version Three"),
            "resume.tex did not fast-forward:\n{forwarded_resume}"
        );

        fs::remove_dir_all(&root).expect("remove temp dir");
    }
}
