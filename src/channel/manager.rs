use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::channel::{Channel, IncomingMessage, OutgoingResponse};

/// Manages multiple channels and merges their message streams.
pub struct ChannelManager {
    channels: HashMap<String, Arc<dyn Channel>>,
    rx: mpsc::UnboundedReceiver<IncomingMessage>,
    tx: Mutex<Option<mpsc::UnboundedSender<IncomingMessage>>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            channels: HashMap::new(),
            rx,
            tx: Mutex::new(Some(tx)),
        }
    }

    /// Register a channel. Call before `start_all()`.
    pub fn add(&mut self, channel: Arc<dyn Channel>) {
        self.channels.insert(channel.name().to_string(), channel);
    }

    /// Start all registered channels. Each channel pushes messages
    /// into the shared sender; the manager receives them from `rx`.
    ///
    /// This is a one-time initialization step and should not be called
    /// concurrently.
    pub async fn start_all(&self) -> anyhow::Result<()> {
        let tx = self
            .tx
            .lock()
            .map_err(|_| anyhow::anyhow!("channel sender lock poisoned"))?
            .take()
            .ok_or_else(|| anyhow::anyhow!("channels already started"))?;
        for channel in self.channels.values() {
            channel.start(tx.clone()).await?;
            tracing::info!(channel = channel.name(), "channel started");
        }
        Ok(())
    }

    /// Receive the next message from any channel.
    pub async fn recv(&mut self) -> Option<IncomingMessage> {
        self.rx.recv().await
    }

    /// Route a response back to the correct channel.
    pub async fn respond(
        &self,
        msg: &IncomingMessage,
        resp: OutgoingResponse,
    ) -> anyhow::Result<()> {
        let channel = self
            .channels
            .get(&msg.channel)
            .ok_or_else(|| anyhow::anyhow!("unknown channel: {}", msg.channel))?;
        channel.respond(msg, resp).await
    }

    /// Shut down all channels gracefully.
    pub async fn shutdown(&self) {
        for channel in self.channels.values() {
            if let Err(e) = channel.shutdown().await {
                tracing::warn!(channel = channel.name(), error = %e, "channel shutdown error");
            }
        }
    }
}
