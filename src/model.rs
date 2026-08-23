use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedConnection {
    pub name: String,
    pub host: String,
    pub username: String,
    pub password_dpapi: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionStore {
    #[serde(default = "store_version")]
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<SavedConnection>,
}

impl Default for ConnectionStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            connections: Vec::new(),
        }
    }
}

pub fn validate_fields(name: &str, host: &str, username: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("名称不能为空");
    }
    if host.trim().is_empty() {
        bail!("IP / 主机名不能为空");
    }
    if username.trim().is_empty() {
        bail!("用户名不能为空");
    }
    if name.contains('\0') || host.contains('\0') || username.contains('\0') {
        bail!("字段不能包含 NUL 字符");
    }
    Ok(())
}

fn store_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::validate_fields;

    #[test]
    fn accepts_normal_connection() {
        assert!(validate_fields("prod", "10.0.0.1", "DOMAIN\\user").is_ok());
    }

    #[test]
    fn rejects_empty_host() {
        assert!(validate_fields("prod", " ", "user").is_err());
    }
}
