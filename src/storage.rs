use crate::model::ConnectionStore;
use anyhow::{Context, Result, bail};
use std::{env, fs, path::PathBuf};

const APP_DIR: &str = "mstsc-mgr-external";
const CONFIG_FILE: &str = "connections.json";

pub fn config_path() -> Result<PathBuf> {
    let base = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("APPDATA"))
        .map(PathBuf::from)
        .context("Windows 用户目录中缺少 LOCALAPPDATA / APPDATA")?;
    Ok(base.join(APP_DIR).join(CONFIG_FILE))
}

pub fn load() -> Result<ConnectionStore> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(ConnectionStore::default());
    }

    let bytes = fs::read(&path)
        .with_context(|| format!("读取配置失败: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(ConnectionStore::default());
    }

    let store: ConnectionStore = serde_json::from_slice(&bytes)
        .with_context(|| format!("解析配置失败: {}", path.display()))?;
    if store.version != 1 {
        bail!("不支持的配置版本: {}", store.version);
    }
    Ok(store)
}

pub fn save(store: &ConnectionStore) -> Result<()> {
    let path = config_path()?;
    let parent = path.parent().context("配置路径缺少父目录")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;

    let bytes = serde_json::to_vec_pretty(store).context("序列化配置失败")?;
    fs::write(&path, bytes)
        .with_context(|| format!("写入配置失败: {}", path.display()))?;
    Ok(())
}
