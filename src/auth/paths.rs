use std::path::PathBuf;

use crate::dirs::project_dirs;

pub fn auth_file_path() -> anyhow::Result<PathBuf> {
    // Allow override via env var for testing.
    if let Ok(path) = std::env::var("XI_AUTH_FILE") {
        return Ok(PathBuf::from(path));
    }
    Ok(project_dirs()?.data_dir().join("auth.toml"))
}
