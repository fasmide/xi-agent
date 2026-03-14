use std::sync::Arc;

use crate::agent::types::ToolRegistry;

pub mod bash;
pub mod edit;
pub mod find;
pub mod read;
pub mod write;

use bash::BashTool;
use edit::EditTool;
use find::FindTool;
use read::ReadFileTool;
use write::WriteTool;

/// Instantiate the four built-in tools and return a populated `ToolRegistry`.
pub fn register_builtin_tools() -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    let tools: Vec<Arc<dyn crate::agent::types::Tool>> = vec![
        Arc::new(ReadFileTool),
        Arc::new(WriteTool),
        Arc::new(EditTool),
        Arc::new(BashTool),
        Arc::new(FindTool),
    ];

    for tool in tools {
        registry.insert(tool.name().to_string(), tool);
    }

    registry
}
