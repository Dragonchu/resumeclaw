mod agent;
mod channel;
mod llm;
mod proxy;
mod tools;
mod workspace;

use std::path::PathBuf;
use std::sync::Arc;

use agent::ResumeAgent;
use channel::cli::CliChannel;
use channel::discord::DiscordChannel;
use channel::manager::ChannelManager;
use tools::ToolRegistry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();
    proxy::init();

    let llm_config = resolve_llm_config();

    // Workspace
    let template_dir = resolve_template_dir(llm_config.uses_dev_examples)?;
    let workspace_dir = std::env::var("WORKSPACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_workspace_dir());
    let initial_template = std::env::var("RESUME_TEMPLATE").ok();
    let workspace = workspace::init(&template_dir, &workspace_dir, initial_template.as_deref())?;
    tracing::info!(path = %workspace.display(), "workspace initialized");

    // LLM
    if let Some(mock_script_path) = llm_config.mock_script_path.as_ref() {
        std::env::set_var("MOCK_LLM_SCRIPT_PATH", mock_script_path);
    }
    if llm_config.mock_repeat_on_exhaustion {
        std::env::set_var("MOCK_LLM_REPEAT_ON_EXHAUSTION", "1");
    } else {
        std::env::remove_var("MOCK_LLM_REPEAT_ON_EXHAUSTION");
    }
    let llm = llm::create_provider(&llm_config.provider, &llm_config.model)?;

    // Tools
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(tools::resume::ReadResume::new(&workspace));
    tool_registry.register(tools::resume::WriteResume::new(&workspace));
    tool_registry.register(tools::resume::CompileResume::new(&workspace));

    // Channels
    let mut channels = ChannelManager::new();
    channels.add(Arc::new(CliChannel));
    tracing::info!("CLI channel enabled for Agent realtime mode");

    if let Some(token) = read_env("DISCORD_BOT_TOKEN") {
        channels.add(Arc::new(DiscordChannel::new(token)));
        tracing::info!("discord channel enabled");
    } else {
        tracing::info!("no optional channel config found; continuing with CLI + Agent realtime mode");
    }

    // Run
    let mut agent = ResumeAgent::new(llm, channels, tool_registry, llm_config.uses_dev_examples);
    agent.run().await
}

struct LlmConfig {
    provider: String,
    model: String,
    mock_script_path: Option<PathBuf>,
    /// When true, the bundled dev mock keeps replying with a simple echo once
    /// the example script is exhausted, instead of surfacing a mock error.
    mock_repeat_on_exhaustion: bool,
    uses_dev_examples: bool,
}

fn resolve_llm_config() -> LlmConfig {
    if let Some(provider) = read_env("LLM_PROVIDER") {
        let model = read_env("LLM_MODEL").unwrap_or_else(|| default_model_for(&provider));
        let mock_script_path = if provider == "mock" {
            let script = read_env("MOCK_LLM_SCRIPT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(default_dev_mock_script_path);
            tracing::info!(path = %script.display(), "mock provider enabled");
            Some(script)
        } else {
            None
        };

        return LlmConfig {
            provider,
            model,
            mock_script_path,
            mock_repeat_on_exhaustion: false,
            uses_dev_examples: false,
        };
    }

    let script = default_dev_mock_script_path();
    tracing::warn!(
        path = %script.display(),
        "LLM provider not configured; falling back to bundled dev mock provider"
    );
    LlmConfig {
        provider: "mock".to_string(),
        model: "mock-dev".to_string(),
        mock_script_path: Some(script),
        mock_repeat_on_exhaustion: true,
        uses_dev_examples: true,
    }
}

fn resolve_template_dir(uses_dev_examples: bool) -> anyhow::Result<PathBuf> {
    if let Some(path) = read_env("RESUME_TEMPLATE_DIR") {
        return Ok(PathBuf::from(path));
    }

    if uses_dev_examples {
        let dev_template_dir = default_dev_template_dir();
        tracing::info!(
            path = %dev_template_dir.display(),
            "template dir not configured; using bundled dev example template"
        );
        return Ok(dev_template_dir);
    }

    bundled_template_dir()
}

fn default_dev_template_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dev/template")
}

fn default_dev_mock_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dev/mock-llm-script.example.json")
}

fn default_model_for(provider: &str) -> String {
    match provider {
        "mock" => "mock-dev".to_string(),
        _ => "deepseek-chat".to_string(),
    }
}

/// Read an environment variable, trimming whitespace and treating blank values as unset.
fn read_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Platform-appropriate default workspace directory.
///
/// - macOS: ~/Library/Application Support/resumeclaw
/// - Linux: $XDG_DATA_HOME/resumeclaw (defaults to ~/.local/share/resumeclaw)
/// - Fallback: ~/.resumeclaw
fn default_workspace_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/resumeclaw");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("resumeclaw");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/resumeclaw");
        }
    }

    // Fallback
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".resumeclaw")
}

fn bundled_template_dir() -> anyhow::Result<PathBuf> {
    if let Some(candidate) = std::env::current_exe()
        .ok()
        .and_then(|exe_path| exe_path.parent().map(|exe_dir| exe_dir.join("templates").join("default")))
        .filter(|candidate| candidate.is_dir())
    {
        return Ok(candidate);
    }

    let manifest_templates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("templates")
        .join("default");
    if manifest_templates.is_dir() {
        return Ok(manifest_templates);
    }

    anyhow::bail!(
        "bundled template directory not found next to the executable or under {}",
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("templates")
            .join("default")
            .display()
    )
}
