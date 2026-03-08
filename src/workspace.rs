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
pub fn init(template_dir: &Path, workspace_dir: &Path) -> anyhow::Result<PathBuf> {
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
        // Try resume2026.tex first, fall back to resume.tex
        let candidates = ["resume2026.tex", "resume.tex", "resume-zh_CN.tex"];
        for name in candidates {
            let src = template_dir.join(name);
            if src.exists() {
                std::fs::copy(&src, &tex_dst)?;
                tracing::info!(template = name, "copied initial resume template");
                break;
            }
        }
    }

    Ok(workspace_dir.to_path_buf())
}
