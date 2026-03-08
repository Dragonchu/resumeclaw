mod agent;
mod channel;
mod llm;
mod proxy;

use std::sync::Arc;

use agent::ResumeAgent;
use channel::cli::CliChannel;
use channel::discord::DiscordChannel;
use channel::manager::ChannelManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();
    proxy::init();

    // LLM
    let provider = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "deepseek".to_string());
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());
    let llm = llm::create_provider(&provider, &model)?;

    // Channels
    let mut channels = ChannelManager::new();

    // Always enable CLI for local interaction.
    channels.add(Arc::new(CliChannel));

    // Enable Discord if token is configured.
    if let Ok(token) = std::env::var("DISCORD_BOT_TOKEN") {
        channels.add(Arc::new(DiscordChannel::new(token)));
        tracing::info!("discord channel enabled");
    }

    // Future: add Feishu, Telegram, etc. here.
    // if let Ok(token) = std::env::var("FEISHU_APP_TOKEN") {
    //     channels.add(Arc::new(FeishuChannel::new(token)));
    // }

    // Run
    let mut agent = ResumeAgent::new(llm, channels);
    agent.run().await
}
