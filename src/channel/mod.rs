pub mod cli;
pub mod discord;
pub mod manager;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// A message received from any channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    /// Unique message id (platform-specific).
    pub id: String,
    /// Which channel sent this ("discord", "feishu", "cli", ...).
    pub channel: String,
    /// User identifier on the platform.
    pub user_id: String,
    /// Display name.
    pub user_name: String,
    /// Message text content.
    pub content: String,
    /// Thread/conversation id for multi-turn replies.
    pub thread_id: Option<String>,
}

/// A response to send back to a channel.
#[derive(Debug, Clone)]
pub struct OutgoingResponse {
    pub content: String,
    pub thread_id: Option<String>,
}

/// Unified channel interface.
///
/// Each platform (Discord, Feishu, CLI, ...) implements this trait.
/// The agent consumes messages from all channels through a single stream.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Channel identifier (e.g. "discord", "feishu").
    fn name(&self) -> &str;

    /// Start receiving messages. Send them into the provided sender.
    ///
    /// This method should spawn background tasks and return immediately.
    /// Messages are pushed into `tx` as they arrive.
    async fn start(&self, tx: mpsc::UnboundedSender<IncomingMessage>) -> anyhow::Result<()>;

    /// Send a response back to the user on this channel.
    async fn respond(&self, msg: &IncomingMessage, resp: OutgoingResponse) -> anyhow::Result<()>;

    /// Graceful shutdown.
    async fn shutdown(&self) -> anyhow::Result<()>;
}
