use std::path::PathBuf;
use std::sync::Arc;

use crate::channel::manager::ChannelManager;
use crate::channel::{IncomingMessage, OutgoingResponse};
use crate::llm::{ChatMessage, LlmProvider};
use crate::tools::ToolRegistry;

const MAX_TOOL_ROUNDS: usize = 10;

pub struct ResumeAgent {
    llm: Arc<dyn LlmProvider>,
    channels: ChannelManager,
    tools: ToolRegistry,
    system_prompt: String,
}

impl ResumeAgent {
    pub fn new(llm: Arc<dyn LlmProvider>, channels: ChannelManager, tools: ToolRegistry) -> Self {
        let system_prompt = r#"你是一个专业的简历助手。你帮助用户修改和优化他们的 LaTeX 简历。

你有以下工具:
- read_resume: 读取当前简历的 LaTeX 源文件
- write_resume: 写入完整的 LaTeX 内容到简历文件
- compile_resume: 使用 xelatex 编译简历为 PDF
- send_resume_email: 将当前编译好的简历 PDF 作为附件发送到指定邮箱，需要提供收件邮箱、邮件标题和正文
- send_resume_email 只能发送到系统配置的允许邮箱列表

工作流程:
1. 收到用户请求后，先用 read_resume 读取当前简历内容
2. 根据用户需求修改内容，用 write_resume 写入修改后的完整 .tex 文件
3. 用 compile_resume 编译为 PDF，PDF 会自动发送给用户
4. 如果用户要求把简历发送到邮箱，确认 PDF 已编译后，再调用 send_resume_email 发送邮件；收件人必须在系统允许列表中

简历使用自定义 LaTeX 类 (resume.cls)，主要命令:
- \name{姓名}
- \basicInfo{联系方式}
- \section{节标题}
- \datedsubsection{\textbf{标题}}{日期范围}
- \role{类型}{职位}
- \begin{itemize} \item 要点 \end{itemize}

注意: write_resume 必须写入完整的 .tex 文件内容，包括 \documentclass 和 \begin{document} 等。
修改后务必 compile_resume 编译并发送 PDF。如果用户明确要求邮件投递，再调用 send_resume_email。"#
            .to_string();

        Self {
            llm,
            channels,
            tools,
            system_prompt,
        }
    }

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

            let (response, attachments) = self.handle(&msg).await;

            let reply = OutgoingResponse {
                content: response,
                thread_id: msg.thread_id.clone(),
                attachments,
            };

            if let Err(e) = self.channels.respond(&msg, reply).await {
                tracing::error!(error = ?e, "failed to respond");
            }
        }

        self.channels.shutdown().await;
        Ok(())
    }

    async fn handle(&self, msg: &IncomingMessage) -> (String, Vec<PathBuf>) {
        let mut messages = vec![
            ChatMessage::system(&self.system_prompt),
            ChatMessage::user(&msg.content),
        ];

        let tool_defs = self.tools.definitions();
        let mut all_attachments = Vec::new();

        for round in 0..MAX_TOOL_ROUNDS {
            let resp = match self
                .llm
                .complete_with_tools(messages.clone(), tool_defs.clone())
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(error = %e, "LLM error");
                    return (format!("Sorry, something went wrong: {e}"), vec![]);
                }
            };

            // No tool calls → final response
            if resp.tool_calls.is_empty() {
                return (resp.content.unwrap_or_default(), all_attachments);
            }

            tracing::info!(
                round,
                tools = resp
                    .tool_calls
                    .iter()
                    .map(|tc| tc.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                "executing tool calls"
            );

            // Add assistant message with tool calls to conversation
            messages.push(ChatMessage::assistant_with_tools(
                resp.content.unwrap_or_default(),
                resp.tool_calls.clone(),
            ));

            // Execute each tool call and collect results
            for tc in &resp.tool_calls {
                let result = self.tools.execute(&tc.name, tc.arguments.clone()).await;
                tracing::info!(
                    tool = %tc.name,
                    attachments = result.attachments.len(),
                    "tool executed"
                );
                all_attachments.extend(result.attachments);
                messages.push(ChatMessage::tool_result(&tc.id, &result.text));
            }
        }

        tracing::warn!("hit max tool rounds ({MAX_TOOL_ROUNDS})");
        (
            "I've reached the maximum number of steps. Please try again with a simpler request."
                .to_string(),
            all_attachments,
        )
    }
}
