use std::{
    fs::{self, OpenOptions},
    sync::OnceLock,
};

use chrono::Local;
use directories::ProjectDirs;
use log::LevelFilter;
use simplelog::{ConfigBuilder, WriteLogger};

static LOG_ENABLED: OnceLock<bool> = OnceLock::new();
static LOG_INITIALIZED: OnceLock<()> = OnceLock::new();

fn is_enabled() -> bool {
    *LOG_ENABLED.get_or_init(|| match std::env::var("PIRS_DEBUG") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v.is_empty() || v == "0" || v == "false" || v == "off")
        }
        Err(_) => false,
    })
}

pub fn init_logging() {
    if !is_enabled() {
        return;
    }

    if LOG_INITIALIZED.get().is_some() {
        return;
    }

    let Some(dirs) = ProjectDirs::from("", "pirs", "pirs") else {
        return;
    };
    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let log_path = dirs.cache_dir().join(format!("pirs-debug-{timestamp}.log"));

    if let Some(parent) = log_path.parent()
        && fs::create_dir_all(parent).is_err()
    {
        return;
    }

    let Ok(file) = OpenOptions::new().create(true).append(true).open(log_path) else {
        return;
    };

    let config = ConfigBuilder::new().set_time_format_rfc3339().build();
    if WriteLogger::init(LevelFilter::Debug, config, file).is_ok() {
        let _ = LOG_INITIALIZED.set(());
    }
}
