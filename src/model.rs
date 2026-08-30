use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionProfile {
    pub id: u64,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub protected_password: String,
    #[serde(default)]
    pub fullscreen: bool,
}

impl ConnectionProfile {
    pub fn endpoint(&self) -> String {
        if self.port == default_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionStore {
    #[serde(default = "format_version")]
    pub format_version: u32,
    #[serde(default)]
    pub connections: Vec<ConnectionProfile>,
}

impl ConnectionStore {
    pub fn next_id(&self) -> u64 {
        self.connections
            .iter()
            .map(|item| item.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }
}

const fn default_port() -> u16 {
    3389
}

const fn format_version() -> u32 {
    1
}
