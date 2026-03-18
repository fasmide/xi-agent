use std::path::PathBuf;

use crate::dirs::project_dirs;

pub fn auth_file_path() -> anyhow::Result<PathBuf> {
    Ok(project_dirs()?.data_dir().join("auth.toml"))
}
