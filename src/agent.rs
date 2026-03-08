use std::sync::Arc;

use crate::channel::{IncomingMessage, OutgoingResponse};
use crate::channel::manager::ChannelManager;
use crate::llm::{ChatMessage, LlmProvider};

pub struct ResumeAgent {
    llm: Arc<dyn LlmProvider>,
    channels: ChannelManager,
    system_prompt: String,
}

impl ResumeAgent {
    pub fn new(llm: Arc<dyn LlmProvider>, channels: ChannelManager) -> Self {
        Self {
            llm,
            channels,
            system_prompt: "You are a helpful resume assistant.".to_string(),
        }
    }

    /// Main event loop: receive messages from all channels, process, respond.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        self.channels.start_all().await?;
        tracing::info!("agent running, waiting for messages...");

        while let Some(msg) = self.channels.recv().await {
            tracing::info!(
                channel = %msg.channel,
                user = %msg.user_name,
                "received: {}",
                msg.content,
            );

            let response = self.handle(&msg).await;

            let reply = OutgoingResponse {
                content: response,
                thread_id: msg.thread_id.clone(),
            };

            if let Err(e) = self.channels.respond(&msg, reply).await {
                tracing::error!(error = ?e, "failed to respond");
            }
        }

        self.channels.shutdown().await;
        Ok(())
    }

    async fn handle(&self, msg: &IncomingMessage) -> String {
        let messages = vec![
            ChatMessage::system(&self.system_prompt),
            ChatMessage::user(&msg.content),
        ];

        match self.llm.complete(messages).await {
            Ok(response) => response,
            Err(e) => {
                tracing::error!(error = %e, "LLM error");
                format!("Sorry, something went wrong: {e}")
            }
        }
    }
}
