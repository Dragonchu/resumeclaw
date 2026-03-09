use async_trait::async_trait;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;

use crate::channel::{Channel, IncomingMessage, OutgoingResponse};

/// Simple CLI channel for local testing.
/// Reads from stdin, prints responses to stdout.
pub struct CliChannel;

enum CliInputMode {
    Normal,
    WriteResumeMultiline { lines: Vec<String> },
}

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
            let mut mode = CliInputMode::Normal;

            while let Ok(Some(line)) = lines.next_line().await {
                match &mut mode {
                    CliInputMode::Normal => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        if trimmed == "/write_resume" {
                            println!(
                                "进入 /write_resume 多行输入模式；输入 /end 提交，输入 /cancel 取消。"
                            );
                            mode = CliInputMode::WriteResumeMultiline { lines: Vec::new() };
                            continue;
                        }

                        if !send_cli_message(&tx, &mut msg_id, trimmed.to_string()) {
                            break;
                        }
                    }
                    CliInputMode::WriteResumeMultiline { lines } => {
                        let trimmed = line.trim();
                        if trimmed == "/cancel" {
                            println!("已取消 /write_resume 多行输入。");
                            mode = CliInputMode::Normal;
                            continue;
                        }

                        if trimmed == "/end" {
                            if lines.is_empty() {
                                println!("未检测到任何内容；继续保持在 /write_resume 多行输入模式。");
                                continue;
                            }

                            let content = lines.join("\n");
                            mode = CliInputMode::Normal;
                            if !send_cli_message(
                                &tx,
                                &mut msg_id,
                                format!("/write_resume {content}"),
                            ) {
                                break;
                            }
                            continue;
                        }

                        lines.push(line);
                    }
                }
            }
        });
        Ok(())
    }

    async fn respond(&self, _msg: &IncomingMessage, resp: OutgoingResponse) -> anyhow::Result<()> {
        println!("{}", resp.content);
        for path in &resp.attachments {
            println!("[attachment: {}]", path.display());
            try_open_attachment(path)?;
        }
        Ok(())
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn try_open_attachment(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg(path)
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run 'open' for {}: {e}", path.display()))?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "'open' exited with status {} for {}",
                status,
                path.display()
            ));
        }
    }

    Ok(())
}

fn send_cli_message(
    tx: &mpsc::UnboundedSender<IncomingMessage>,
    msg_id: &mut u64,
    content: String,
) -> bool {
    *msg_id += 1;
    let incoming = IncomingMessage {
        id: msg_id.to_string(),
        channel: "cli".to_string(),
        user_id: "local".to_string(),
        user_name: "user".to_string(),
        content,
        thread_id: None,
    };
    tx.send(incoming).is_ok()
}
