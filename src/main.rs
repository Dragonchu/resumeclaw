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

    // Workspace
    let template_dir = std::env::var("RESUME_TEMPLATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| bundled_template_dir());
    let workspace_dir = std::env::var("WORKSPACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_workspace_dir());
    let initial_template = std::env::var("RESUME_TEMPLATE").ok();
    let workspace = workspace::init(&template_dir, &workspace_dir, initial_template.as_deref())?;
    tracing::info!(path = %workspace.display(), "workspace initialized");

    // LLM
    let provider = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "deepseek".to_string());
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());
    let llm = llm::create_provider(&provider, &model)?;

    // Tools
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(tools::resume::ReadResume::new(&workspace));
    tool_registry.register(tools::resume::WriteResume::new(&workspace));
    tool_registry.register(tools::resume::CompileResume::new(&workspace));

    // Channels
    let mut channels = ChannelManager::new();
    channels.add(Arc::new(CliChannel));

    if let Ok(token) = std::env::var("DISCORD_BOT_TOKEN") {
        channels.add(Arc::new(DiscordChannel::new(token)));
        tracing::info!("discord channel enabled");
    }

    // Run
    let mut agent = ResumeAgent::new(llm, channels, tool_registry);
    agent.run().await
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

fn bundled_template_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("templates")
        .join("default")
}
