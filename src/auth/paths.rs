use std::path::PathBuf;

use anyhow::Context;
use directories::ProjectDirs;

pub fn config_dir() -> anyhow::Result<PathBuf> {
    let dirs = ProjectDirs::from("", "pirs", "pirs")
        .context("Could not resolve platform config directory for pirs")?;
    Ok(dirs.config_dir().to_path_buf())
}

pub fn auth_file_path() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("auth.toml"))
}
