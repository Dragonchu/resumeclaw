//! Workspace management for resume editing.
//!
//! Initializes a workspace directory with LaTeX template files from the
//! resume template directory. The .tex file in the workspace is managed
//! by the agent's tools.

use std::path::{Path, PathBuf};

/// Supporting files needed from the template directory.
const TEMPLATE_FILES: &[&str] = &[
    "resume.cls",
    "zh_CN-Adobefonts_external.sty",
    "zh_CN-Adobefonts_internal.sty",
    "NotoSansSC_external.sty",
    "NotoSerifCJKsc_external.sty",
    "linespacing_fix.sty",
    "fontawesome.sty",
    "fontawesomesymbols-xeluatex.tex",
    "fontawesomesymbols-generic.tex",
    "fontawesomesymbols-pdftex.tex",
];

/// Initialize the workspace directory.
///
/// - Copies supporting LaTeX files (.cls, .sty, etc.) from template_dir
/// - Symlinks the fonts/ directory
/// - Copies the initial .tex file if none exists
///
/// Returns the workspace path.
pub fn init(
    template_dir: &Path,
    workspace_dir: &Path,
    initial_template: Option<&str>,
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(workspace_dir)?;

    // Copy supporting files (skip if already present)
    for file in TEMPLATE_FILES {
        let src = template_dir.join(file);
        let dst = workspace_dir.join(file);
        if src.exists() && !dst.try_exists().unwrap_or(false) {
            std::fs::copy(&src, &dst)?;
            tracing::debug!(file, "copied template file");
        }
    }

    // Symlink fonts directory (avoid copying large font files)
    let fonts_src = template_dir.join("fonts");
    let fonts_dst = workspace_dir.join("fonts");
    if fonts_src.exists() {
        // Remove stale symlink or existing entry, then recreate
        if fonts_dst.symlink_metadata().is_ok() {
            let _ = std::fs::remove_file(&fonts_dst);
        }
        if !fonts_dst.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::canonicalize(&fonts_src)?, &fonts_dst)?;
            tracing::debug!("symlinked fonts directory");
        }
    }

    // Copy initial resume template if no resume.tex exists yet
    let tex_dst = workspace_dir.join("resume.tex");
    if !tex_dst.exists() {
        let mut copied = false;
        for name in template_candidates(template_dir, initial_template)? {
            let src = template_dir.join(&name);
            if src.exists() {
                std::fs::copy(&src, &tex_dst)?;
                tracing::info!(template = name, "copied initial resume template");
                copied = true;
                break;
            }
        }

        if !copied {
            anyhow::bail!(
                "no resume template found in {} (requested: {:?})",
                template_dir.display(),
                initial_template,
            );
        }
    }

    Ok(workspace_dir.to_path_buf())
}

fn template_candidates(
    template_dir: &Path,
    initial_template: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let mut candidates = Vec::new();

    if let Some(name) = initial_template.filter(|name| !name.trim().is_empty()) {
        let name = validate_template_name(name)?;
        push_unique(&mut candidates, name);
    }

    let mut discovered = discover_templates(template_dir)?;
    for name in ["resume2026.tex", "resume.tex", "resume-zh_CN.tex"] {
        if discovered
            .binary_search_by(|candidate| candidate.as_str().cmp(name))
            .is_ok()
        {
            push_unique(&mut candidates, name.to_string());
        }
    }

    for name in discovered.drain(..) {
        push_unique(&mut candidates, name);
    }

    Ok(candidates)
}

fn discover_templates(template_dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut templates = std::fs::read_dir(template_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_file() {
                return None;
            }

            let name = entry.file_name();
            let name = name.to_str()?;
            if Path::new(name).extension().and_then(|ext| ext.to_str()) == Some("tex") {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    templates.sort();
    Ok(templates)
}

fn validate_template_name(name: &str) -> anyhow::Result<String> {
    let path = Path::new(name);
    let mut components = path.components();
    let is_plain_file = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    let has_tex_extension = path.extension().and_then(|ext| ext.to_str()) == Some("tex");

    if is_plain_file && has_tex_extension {
        Ok(name.to_string())
    } else {
        anyhow::bail!("invalid template name: {name}");
    }
}

fn push_unique(candidates: &mut Vec<String>, name: String) {
    if !candidates.iter().any(|candidate| candidate == &name) {
        candidates.push(name);
    }
}

#[cfg(test)]
mod tests {
    use super::{discover_templates, init, template_candidates, validate_template_name};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("resumeclaw-{prefix}-{unique}"));
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).expect("write test file");
    }

    #[test]
    fn copies_default_english_template() {
        let template_dir = TestDir::new("template-en");
        let workspace_dir = TestDir::new("workspace-en");

        write_file(template_dir.path(), "resume.cls", "class");
        write_file(template_dir.path(), "resume.tex", "english");
        write_file(template_dir.path(), "resume-zh_CN.tex", "chinese");

        init(template_dir.path(), workspace_dir.path(), None).expect("initialize workspace");

        let resume =
            std::fs::read_to_string(workspace_dir.path().join("resume.tex")).expect("read resume");
        assert_eq!(resume, "english");
    }

    #[test]
    fn copies_requested_template_when_available() {
        let template_dir = TestDir::new("template-zh");
        let workspace_dir = TestDir::new("workspace-zh");

        write_file(template_dir.path(), "resume.cls", "class");
        write_file(template_dir.path(), "resume.tex", "english");
        write_file(template_dir.path(), "resume-zh_CN.tex", "chinese");

        init(
            template_dir.path(),
            workspace_dir.path(),
            Some("resume-zh_CN.tex"),
        )
        .expect("initialize workspace");

        let resume =
            std::fs::read_to_string(workspace_dir.path().join("resume.tex")).expect("read resume");
        assert_eq!(resume, "chinese");
    }

    #[test]
    fn keeps_existing_resume_tex() {
        let template_dir = TestDir::new("template-existing");
        let workspace_dir = TestDir::new("workspace-existing");

        write_file(template_dir.path(), "resume.cls", "class");
        write_file(template_dir.path(), "resume.tex", "english");
        write_file(workspace_dir.path(), "resume.tex", "custom");

        init(template_dir.path(), workspace_dir.path(), None).expect("initialize workspace");

        let resume =
            std::fs::read_to_string(workspace_dir.path().join("resume.tex")).expect("read resume");
        assert_eq!(resume, "custom");
    }

    #[test]
    fn treats_any_tex_file_as_a_template() {
        let template_dir = TestDir::new("template-list");

        write_file(template_dir.path(), "resume.cls", "class");
        write_file(template_dir.path(), "custom.tex", "custom");
        write_file(template_dir.path(), "resume.tex", "english");
        write_file(template_dir.path(), "notes.txt", "ignore");

        let templates = discover_templates(template_dir.path()).expect("discover templates");
        assert_eq!(templates, vec!["custom.tex", "resume.tex"]);
    }

    #[test]
    fn requested_template_is_prioritized_even_for_custom_tex_files() {
        let template_dir = TestDir::new("template-custom");
        let workspace_dir = TestDir::new("workspace-custom");

        write_file(template_dir.path(), "resume.cls", "class");
        write_file(template_dir.path(), "resume.tex", "english");
        write_file(template_dir.path(), "portfolio.tex", "portfolio");

        init(
            template_dir.path(),
            workspace_dir.path(),
            Some("portfolio.tex"),
        )
        .expect("initialize workspace");

        let resume =
            std::fs::read_to_string(workspace_dir.path().join("resume.tex")).expect("read resume");
        assert_eq!(resume, "portfolio");
    }

    #[test]
    fn rejects_template_names_with_paths() {
        assert!(validate_template_name("../resume.tex").is_err());
        assert!(validate_template_name("nested/resume.tex").is_err());
        assert!(validate_template_name("/tmp/resume.tex").is_err());
        assert!(validate_template_name("resume.txt").is_err());
    }

    #[test]
    fn preferred_defaults_stay_ahead_of_other_tex_templates() {
        let template_dir = TestDir::new("template-order");

        write_file(template_dir.path(), "resume.cls", "class");
        write_file(template_dir.path(), "aaa.tex", "a");
        write_file(template_dir.path(), "resume.tex", "english");
        write_file(template_dir.path(), "resume-zh_CN.tex", "chinese");

        let candidates = template_candidates(template_dir.path(), None).expect("list candidates");
        assert_eq!(
            candidates,
            vec![
                "resume.tex".to_string(),
                "resume-zh_CN.tex".to_string(),
                "aaa.tex".to_string(),
            ]
        );
    }
}
