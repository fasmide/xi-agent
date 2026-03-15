use std::{env, ffi::OsStr, fs, path::PathBuf};

use anyhow::Context;

#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct PirsConfig {
    pub provider: Option<String>,

    #[serde(default)]
    pub openai: OpenAiConfig,
    #[serde(default)]
    pub copilot: CopilotConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub codex: CodexConfig,
}

#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct OpenAiConfig {
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct CopilotConfig {
    pub model: Option<String>,
}

#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct OllamaConfig {
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct CodexConfig {
    pub model: Option<String>,
}

impl PirsConfig {
    /// Load from $XDG_CONFIG_HOME/pirs/config.toml (or ~/.config/pirs/config.toml).
    /// Missing file is not an error and returns `Default`.
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        Self::from_toml_str(&raw)
            .with_context(|| format!("Failed to parse TOML config file: {}", path.display()))
    }

    pub fn from_toml_str(raw: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str::<Self>(raw)?)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let body = toml::to_string_pretty(self)?;
        fs::write(&path, body)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        Ok(())
    }
}

pub fn config_path() -> anyhow::Result<PathBuf> {
    config_path_from(
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

fn config_path_from(xdg_home: Option<&OsStr>, home: Option<&OsStr>) -> anyhow::Result<PathBuf> {
    if let Some(xdg_home) = xdg_home
        && !xdg_home.is_empty()
    {
        return Ok(PathBuf::from(xdg_home).join("pirs").join("config.toml"));
    }

    let home = home.context("Could not resolve HOME for config path")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("pirs")
        .join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::{PirsConfig, config_path_from};
    use std::ffi::OsStr;

    #[test]
    fn parses_full_config_toml() {
        let raw = r#"
provider = "openai"

[openai]
api_key = "sk-test"
model = "gpt-4o-mini"

[copilot]
model = "gpt-4o"

[codex]
model = "gpt-5"

[ollama]
base_url = "http://localhost:11434"
model = "llama3.1"
"#;

        let cfg = PirsConfig::from_toml_str(raw).expect("config parses");

        assert_eq!(cfg.provider.as_deref(), Some("openai"));
        assert_eq!(cfg.openai.api_key.as_deref(), Some("sk-test"));
        assert_eq!(cfg.openai.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(cfg.copilot.model.as_deref(), Some("gpt-4o"));
        assert_eq!(cfg.codex.model.as_deref(), Some("gpt-5"));
        assert_eq!(
            cfg.ollama.base_url.as_deref(),
            Some("http://localhost:11434")
        );
        assert_eq!(cfg.ollama.model.as_deref(), Some("llama3.1"));
    }

    #[test]
    fn config_path_prefers_xdg() {
        let path = config_path_from(Some(OsStr::new("/tmp/xdg")), Some(OsStr::new("/tmp/home")))
            .expect("path resolves");
        assert_eq!(path, std::path::Path::new("/tmp/xdg/pirs/config.toml"));
    }

    #[test]
    fn config_path_falls_back_to_home() {
        let path = config_path_from(None, Some(OsStr::new("/home/alice"))).expect("path resolves");
        assert_eq!(
            path,
            std::path::Path::new("/home/alice/.config/pirs/config.toml")
        );
    }
}
