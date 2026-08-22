use crate::platform::RuntimeSettings;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tracing_subscriber::EnvFilter;

pub const LOG_FILE_NAME: &str = "mstsc-mgr.log";

#[derive(Clone)]
struct DynamicLogWriter {
    file: Arc<Mutex<Option<File>>>,
    settings: RuntimeSettings,
}

impl Write for DynamicLogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let enabled = self
            .settings
            .read()
            .map(|settings| settings.logging_enabled)
            .unwrap_or(true);
        if !enabled {
            return Ok(buffer.len());
        }

        let Ok(mut file) = self.file.lock() else {
            return Ok(buffer.len());
        };
        match file.as_mut() {
            Some(file) => file.write(buffer),
            None => Ok(buffer.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let enabled = self
            .settings
            .read()
            .map(|settings| settings.logging_enabled)
            .unwrap_or(true);
        if !enabled {
            return Ok(());
        }

        let Ok(mut file) = self.file.lock() else {
            return Ok(());
        };
        match file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

pub fn program_log_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    Some(directory.join(LOG_FILE_NAME))
}

pub fn init(settings: RuntimeSettings) -> Option<PathBuf> {
    let path = program_log_path()?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let file_available = file.is_some();
    let shared_file = Arc::new(Mutex::new(file));
    let writer = DynamicLogWriter {
        file: shared_file,
        settings,
    };

    let install_result = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("mstsc_mgr=info"))
        .with_ansi(false)
        .with_thread_ids(true)
        .with_writer(move || writer.clone())
        .try_init();
    if install_result.is_err() || !file_available {
        return None;
    }
    Some(path)
}
