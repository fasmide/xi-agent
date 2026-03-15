use std::collections::HashMap;

use serde::{Deserialize, Serialize};

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub providers: HashMap<String, ProviderCredentials>,
}

impl Default for AuthFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            providers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ProviderCredentials {
    #[serde(rename = "copilot")]
    Copilot {
        access_token: String,
        refresh_token: String,
        expires_at: i64,
        #[serde(default)]
        base_url: Option<String>,
    },
    #[serde(rename = "codex")]
    Codex {
        access_token: String,
        refresh_token: String,
        expires_at: i64,
        account_id: String,
    },
}

#[derive(Debug, Clone)]
pub struct CopilotCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodexCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub account_id: String,
}
