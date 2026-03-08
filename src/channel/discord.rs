use std::sync::Arc;

use async_trait::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use tokio::sync::mpsc;

use crate::channel::{Channel, IncomingMessage, OutgoingResponse};

/// Discord channel using serenity gateway (bot WebSocket connection).
pub struct DiscordChannel {
    token: String,
    /// Serenity HTTP client, available after start().
    http: tokio::sync::OnceCell<Arc<serenity::http::Http>>,
}

impl DiscordChannel {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            http: tokio::sync::OnceCell::new(),
        }
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    async fn start(&self, tx: mpsc::UnboundedSender<IncomingMessage>) -> anyhow::Result<()> {
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let mut client = Client::builder(&self.token, intents)
            .event_handler(Handler { tx })
            .await?;

        // Store HTTP client for respond().
        let _ = self.http.set(client.cache_and_http.http.clone());

        // Spawn the gateway connection in background.
        tokio::spawn(async move {
            if let Err(e) = client.start().await {
                tracing::error!(error = %e, "discord gateway error");
            }
        });

        Ok(())
    }

    async fn respond(&self, msg: &IncomingMessage, resp: OutgoingResponse) -> anyhow::Result<()> {
        let http = self.http.get().ok_or_else(|| {
            anyhow::anyhow!("discord not started yet")
        })?;

        let channel_id: u64 = msg
            .thread_id
            .as_deref()
            .unwrap_or(&msg.id)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid discord channel id"))?;

        let channel = serenity::model::id::ChannelId(channel_id);
        channel.say(http, &resp.content).await?;

        Ok(())
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        // serenity handles graceful disconnect on drop.
        Ok(())
    }
}

/// Serenity event handler that forwards messages to the channel manager.
struct Handler {
    tx: mpsc::UnboundedSender<IncomingMessage>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: Message) {
        // Ignore bot messages to prevent loops.
        if msg.author.bot {
            return;
        }

        let incoming = IncomingMessage {
            id: msg.id.to_string(),
            channel: "discord".to_string(),
            user_id: msg.author.id.to_string(),
            user_name: msg.author.name.clone(),
            content: msg.content.clone(),
            thread_id: Some(msg.channel_id.to_string()),
        };

        if let Err(e) = self.tx.send(incoming) {
            tracing::error!(error = %e, "failed to forward discord message");
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        tracing::info!(user = %ready.user.name, "discord bot connected");
    }
}
