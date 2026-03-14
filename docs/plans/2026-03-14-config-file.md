# Config File

**Date:** 2026-03-14  
**Status:** Planned  
**Priority:** Medium

## Problem

All configuration is currently env-var only. Users must set `PIRS_PROVIDER`,
`OPENAI_API_KEY`, `COPILOT_MODEL`, etc. on every launch or in their shell
profile. There is no way to persist a preferred provider, model, or API key
without shell-level configuration. This makes the first-run experience poor
and multi-provider workflows awkward.

## Goal

Add an optional `~/.config/pirs/config.toml` that can store API keys, the
default provider, and per-provider default models. Env vars and CLI flags
override the config file, so existing workflows are unaffected.

## Config file format

```toml
# ~/.config/pirs/config.toml

# Default provider if PIRS_PROVIDER and --provider are not set.
provider = "openai"

[openai]
api_key = "sk-…"
model   = "gpt-4o-mini"

[ollama]
base_url = "http://gpu-box:11434"
model    = "llama3.2"

[copilot]
# auth is still read from ~/.pi/agent/auth.json; no key needed here.
model = "gpt-4o"
```

All keys are optional. A missing file is silently ignored (not an error).

## Precedence (highest to lowest)

1. CLI flag (`--provider`, `--model`)
2. Env var (`PIRS_PROVIDER`, `OPENAI_API_KEY`, `COPILOT_MODEL`, …)
3. Config file (`~/.config/pirs/config.toml`)
4. Hard-coded defaults in `provider.rs`

## New module: `src/config.rs`

```rust
/// Parsed representation of ~/.config/pirs/config.toml.
/// All fields are optional; absent values fall through to env vars / defaults.
#[derive(Debug, Default, serde::Deserialize)]
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

#[derive(Debug, Default, serde::Deserialize)]
pub struct OpenAiConfig {
    pub api_key: Option<String>,
    pub model:   Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct CopilotConfig {
    pub model: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct OllamaConfig {
    pub base_url: Option<String>,
    pub model:    Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct CodexConfig {
    pub model: Option<String>,
}

impl PirsConfig {
    /// Load from ~/.config/pirs/config.toml.
    /// Returns Default if the file does not exist.
    pub fn load() -> anyhow::Result<Self> { … }
}
```

`toml` crate added to `Cargo.toml` (`toml = { version = "0.8", features = ["parse"] }`).

## Changes to `provider.rs`

`build_provider` gains a `config: &PirsConfig` parameter. Each provider
constructor reads from the config struct before falling back to env vars.

```rust
pub fn build_provider(
    kind: &ProviderKind,
    model: &str,
    config: &PirsConfig,
) -> anyhow::Result<Arc<dyn LlmProvider + Send + Sync>>
```

## Changes to `main.rs`

Load config once at startup before any provider construction:

```rust
let config = PirsConfig::load().unwrap_or_default();
```

Pass it through to `build_provider` and to the initial provider/model
resolution logic.

## Config file location

Follow the XDG spec: `$XDG_CONFIG_HOME/pirs/config.toml`, falling back to
`~/.config/pirs/config.toml`.

## Implementation Tasks

1. Add `toml` to `Cargo.toml`.
2. Implement `src/config.rs` with `PirsConfig` and `PirsConfig::load()`.
3. Update `provider.rs`: `build_provider` takes `&PirsConfig`; each arm reads
   from config before env vars.
4. Update `main.rs`: load config, thread it through provider construction and
   initial provider/model resolution.
5. Update README config table to document the file format.
