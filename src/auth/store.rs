use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::auth::paths::auth_file_path;
use crate::auth::types::{AuthFile, CodexCredentials, CopilotCredentials, ProviderCredentials};

pub struct AuthStore {
    path: PathBuf,
    data: AuthFile,
}

impl AuthStore {
    pub fn load_default() -> anyhow::Result<Self> {
        let path = auth_file_path()?;
        Self::load(path)
    }

    pub fn load(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let data = if path.exists() {
            let text = fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", path.display(), e))?;
            serde_json::from_str::<AuthFile>(&text)
                .map_err(|e| anyhow::anyhow!("Cannot parse {}: {}", path.display(), e))?
        } else {
            AuthFile::default()
        };

        Ok(Self { path, data })
    }

    pub fn get_copilot(&self) -> Option<CopilotCredentials> {
        match self.data.providers.get("copilot") {
            Some(ProviderCredentials::Copilot {
                access_token,
                refresh_token,
                expires_at,
                base_url,
            }) => Some(CopilotCredentials {
                access_token: access_token.clone(),
                refresh_token: refresh_token.clone(),
                expires_at: *expires_at,
                base_url: base_url.clone(),
            }),
            _ => None,
        }
    }

    pub fn get_codex(&self) -> Option<CodexCredentials> {
        match self.data.providers.get("codex") {
            Some(ProviderCredentials::Codex {
                access_token,
                refresh_token,
                expires_at,
                account_id,
            }) => Some(CodexCredentials {
                access_token: access_token.clone(),
                refresh_token: refresh_token.clone(),
                expires_at: *expires_at,
                account_id: account_id.clone(),
            }),
            _ => None,
        }
    }

    pub fn set_copilot(&mut self, creds: CopilotCredentials) {
        self.data.providers.insert(
            "copilot".to_string(),
            ProviderCredentials::Copilot {
                access_token: creds.access_token,
                refresh_token: creds.refresh_token,
                expires_at: creds.expires_at,
                base_url: creds.base_url,
            },
        );
    }

    pub fn set_codex(&mut self, creds: CodexCredentials) {
        self.data.providers.insert(
            "codex".to_string(),
            ProviderCredentials::Codex {
                access_token: creds.access_token,
                refresh_token: creds.refresh_token,
                expires_at: creds.expires_at,
                account_id: creds.account_id,
            },
        );
    }

    pub fn save(&self) -> anyhow::Result<()> {
        save_atomic(&self.path, &self.data)
    }
}

fn save_atomic(path: &Path, data: &AuthFile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("Cannot create {}: {}", parent.display(), e))?;
    }

    let serialized = serde_json::to_string_pretty(data)
        .map_err(|e| anyhow::anyhow!("Cannot serialize auth file: {}", e))?;

    let tmp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp_path)
            .map_err(|e| anyhow::anyhow!("Cannot create {}: {}", tmp_path.display(), e))?;
        file.write_all(serialized.as_bytes())
            .map_err(|e| anyhow::anyhow!("Cannot write {}: {}", tmp_path.display(), e))?;
        file.sync_all()
            .map_err(|e| anyhow::anyhow!("Cannot sync {}: {}", tmp_path.display(), e))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&tmp_path, path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot rename {} to {}: {}",
            tmp_path.display(),
            path.display(),
            e
        )
    })?;

    Ok(())
}
