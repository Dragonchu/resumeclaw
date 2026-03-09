//! Tool system for the resume agent.
//!
//! Tools are functions the LLM can call to interact with the workspace.

pub mod email;
pub mod resume;

use std::path::PathBuf;

use async_trait::async_trait;

use crate::llm::provider::ToolDefinition;

/// Result of executing a tool.
pub struct ToolResult {
    /// Text result to send back to the LLM.
    pub text: String,
    /// File attachments produced (e.g. compiled PDF).
    pub attachments: Vec<PathBuf>,
}

/// Trait for tool implementations.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// The tool definition (name, description, parameters schema).
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given arguments.
    async fn execute(&self, args: serde_json::Value) -> ToolResult;
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: Vec<Box<dyn ToolHandler>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: vec![] }
    }

    pub fn register(&mut self, tool: impl ToolHandler + 'static) {
        self.tools.push(Box::new(tool));
    }

    /// Get all tool definitions for passing to the LLM.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|t| t.definition()).collect()
    }

    /// Get one tool definition by name.
    pub fn definition(&self, name: &str) -> Option<ToolDefinition> {
        self.tools.iter().find_map(|tool| {
            let definition = tool.definition();
            (definition.name == name).then_some(definition)
        })
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, args: serde_json::Value) -> ToolResult {
        for tool in &self.tools {
            if tool.definition().name == name {
                return tool.execute(args).await;
            }
        }
        ToolResult {
            text: format!("Unknown tool: {name}"),
            attachments: vec![],
        }
    }
}
