use std::sync::Arc;

use crate::agent::types::ToolRegistry;

pub mod ask_user;
pub mod bash;
pub mod edit;
pub mod find;
pub mod read;
pub mod write;

use ask_user::AskUserTool;
use bash::BashTool;
use edit::EditTool;
use find::FindTool;
use read::ReadFileTool;
use write::WriteTool;

use crate::agent::types::AskRequestTx;

/// Instantiate the built-in tools and return a populated `ToolRegistry`.
pub fn register_builtin_tools(ask_tx: Option<AskRequestTx>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    let tools: Vec<Arc<dyn crate::agent::types::Tool>> = vec![
        Arc::new(ReadFileTool),
        Arc::new(WriteTool),
        Arc::new(EditTool),
        Arc::new(BashTool),
        Arc::new(FindTool),
        Arc::new(AskUserTool::new(ask_tx)),
    ];

    for tool in tools {
        registry.insert(tool.name().to_string(), tool);
    }

    registry
}
