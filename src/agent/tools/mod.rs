use std::sync::Arc;

use crate::agent::types::ToolRegistry;

pub mod ask_user;
#[cfg(not(target_os = "windows"))]
pub mod bash;
#[cfg(target_os = "windows")]
pub mod cmd;
pub mod edit;
pub mod find;
#[cfg(target_os = "windows")]
pub mod powershell;
pub mod read;
pub mod write;

use ask_user::AskUserTool;
#[cfg(not(target_os = "windows"))]
use bash::BashTool;
#[cfg(target_os = "windows")]
use cmd::CmdTool;
use edit::EditTool;
use find::FindTool;
#[cfg(target_os = "windows")]
use powershell::PowerShellTool;
use read::ReadFileTool;
use write::WriteTool;

use crate::agent::types::AskRequestTx;

/// Instantiate the built-in tools and return a populated `ToolRegistry`.
pub fn register_builtin_tools(ask_tx: Option<AskRequestTx>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    let mut tools: Vec<Arc<dyn crate::agent::types::Tool>> = vec![
        Arc::new(ReadFileTool),
        Arc::new(WriteTool),
        Arc::new(EditTool),
        Arc::new(FindTool),
        Arc::new(AskUserTool::new(ask_tx)),
    ];

    #[cfg(target_os = "windows")]
    {
        tools.push(Arc::new(PowerShellTool));
        tools.push(Arc::new(CmdTool));
    }

    #[cfg(not(target_os = "windows"))]
    {
        tools.push(Arc::new(BashTool));
    }

    for tool in tools {
        registry.insert(tool.name().to_string(), tool);
    }

    registry
}
