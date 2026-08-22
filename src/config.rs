use crate::{
    crypto,
    domain::{AppSettings, VaultPayload},
};
use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub settings: PathBuf,
    pub vault: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let base = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .context("LOCALAPPDATA is unavailable")?;
        let root = base.join("mstsc-mgr");
        Ok(Self {
            settings: root.join("settings.json"),
            vault: root.join("vault.dpapi"),
            root,
        })
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))
    }
}

pub fn load_settings(paths: &AppPaths) -> Result<AppSettings> {
    if !paths.settings.exists() {
        return Ok(AppSettings::default());
    }
    let bytes = fs::read(&paths.settings)
        .with_context(|| format!("failed to read {}", paths.settings.display()))?;
    serde_json::from_slice(&bytes).context("invalid settings.json")
}

pub fn save_settings(paths: &AppPaths, settings: &AppSettings) -> Result<()> {
    paths.ensure()?;
    let bytes = serde_json::to_vec_pretty(settings)?;
    atomic_write(&paths.settings, &bytes)
}

pub fn load_vault(paths: &AppPaths) -> Result<VaultPayload> {
    if !paths.vault.exists() {
        return Ok(VaultPayload::default());
    }
    let encrypted = fs::read(&paths.vault)
        .with_context(|| format!("failed to read {}", paths.vault.display()))?;
    let clear = crypto::unprotect(&encrypted).context("unable to decrypt local vault")?;
    let vault: VaultPayload = serde_json::from_slice(&clear).context("invalid vault payload")?;
    if vault.format_version != 1 {
        bail!("unsupported vault format version {}", vault.format_version);
    }
    Ok(vault)
}

pub fn save_vault(paths: &AppPaths, vault: &VaultPayload) -> Result<()> {
    paths.ensure()?;
    let clear = serde_json::to_vec(vault)?;
    let encrypted = crypto::protect(&clear).context("unable to encrypt local vault")?;
    atomic_write(&paths.vault, &encrypted)
}

pub fn export_vault(vault: &VaultPayload, destination: &Path) -> Result<()> {
    let clear = serde_json::to_vec(vault)?;
    let encrypted = crypto::protect(&clear).context("unable to encrypt export")?;
    atomic_write(destination, &encrypted)
}

pub fn import_vault(source: &Path) -> Result<VaultPayload> {
    let encrypted =
        fs::read(source).with_context(|| format!("failed to read {}", source.display()))?;
    let clear = crypto::unprotect(&encrypted)
        .context("unable to decrypt import; v0.1 exports require the same Windows user profile")?;
    let vault: VaultPayload = serde_json::from_slice(&clear).context("invalid imported vault")?;
    if vault.format_version != 1 {
        bail!(
            "unsupported imported vault format version {}",
            vault.format_version
        );
    }
    Ok(vault)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to replace {}", path.display()))?;
    }
    fs::rename(&tmp, path).with_context(|| format!("failed to commit {}", path.display()))
}
