use crate::model::ConnectionStore;
use anyhow::{Context, Result};
use std::{fs, path::PathBuf};

pub fn load() -> Result<ConnectionStore> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(ConnectionStore::default());
    }
    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn save(store: &ConnectionStore) -> Result<()> {
    let path = store_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(store).context("failed to serialize connections")?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).with_context(|| format!("failed to write {}", temp.display()))?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to replace {}", path.display()))?;
    }
    fs::rename(&temp, &path).with_context(|| format!("failed to commit {}", path.display()))?;
    Ok(())
}

pub fn store_path() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not defined")?;
    Ok(PathBuf::from(local)
        .join("mstsc-mgr-external")
        .join("connections.json"))
}
