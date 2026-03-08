use async_trait::async_trait;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;

use crate::channel::{Channel, IncomingMessage, OutgoingResponse};

/// Simple CLI channel for local testing.
/// Reads from stdin, prints responses to stdout.
pub struct CliChannel;

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    async fn start(&self, tx: mpsc::UnboundedSender<IncomingMessage>) -> anyhow::Result<()> {
        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let reader = tokio::io::BufReader::new(stdin);
            let mut lines = reader.lines();
            let mut msg_id: u64 = 0;

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                msg_id += 1;
                let incoming = IncomingMessage {
                    id: msg_id.to_string(),
                    channel: "cli".to_string(),
                    user_id: "local".to_string(),
                    user_name: "user".to_string(),
                    content: line,
                    thread_id: None,
                };
                if tx.send(incoming).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    async fn respond(&self, _msg: &IncomingMessage, resp: OutgoingResponse) -> anyhow::Result<()> {
        println!("{}", resp.content);
        for path in &resp.attachments {
            println!("[attachment: {}]", path.display());
        }
        Ok(())
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
