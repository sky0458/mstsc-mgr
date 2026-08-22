use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedConnection {
    pub id: u64,
    pub name: String,
    pub host: String,
    #[serde(default = "default_rdp_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub mstsc_args: Vec<String>,
}

impl fmt::Debug for SavedConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SavedConnection")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("mstsc_args", &self.mstsc_args)
            .finish()
    }
}

impl SavedConnection {
    pub fn endpoint(&self) -> String {
        if self.port == default_rdp_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn credential_target(&self) -> String {
        format!("TERMSRV/{}", self.host)
    }
}

const fn default_rdp_port() -> u16 {
    3389
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeepAliveInput {
    #[default]
    MouseMove,
    ShiftKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub floating_controller: bool,
    #[serde(default)]
    pub always_show_tabs: bool,
    #[serde(default = "default_true")]
    pub global_hotkeys: bool,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_true")]
    pub logging_enabled: bool,
    #[serde(default)]
    pub keepalive_enabled: bool,
    #[serde(default = "default_keepalive_seconds")]
    pub keepalive_interval_seconds: u64,
    #[serde(default)]
    pub keepalive_input: KeepAliveInput,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            floating_controller: true,
            always_show_tabs: false,
            global_hotkeys: true,
            close_to_tray: true,
            logging_enabled: true,
            keepalive_enabled: false,
            keepalive_interval_seconds: default_keepalive_seconds(),
            keepalive_input: KeepAliveInput::MouseMove,
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_keepalive_seconds() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultPayload {
    pub format_version: u32,
    #[serde(default)]
    pub connections: Vec<SavedConnection>,
}

impl Default for VaultPayload {
    fn default() -> Self {
        Self {
            format_version: 1,
            connections: Vec::new(),
        }
    }
}

impl VaultPayload {
    pub fn next_id(&self) -> u64 {
        self.connections
            .iter()
            .map(|item| item.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MstscWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> SavedConnection {
        SavedConnection {
            id: 1,
            name: "dev".into(),
            host: "10.0.0.8".into(),
            port: 3389,
            username: "alice".into(),
            password: "secret".into(),
            mstsc_args: vec!["/f".into()],
        }
    }

    #[test]
    fn endpoint_omits_default_port() {
        assert_eq!(connection().endpoint(), "10.0.0.8");
    }

    #[test]
    fn endpoint_includes_custom_port() {
        let mut item = connection();
        item.port = 3390;
        assert_eq!(item.endpoint(), "10.0.0.8:3390");
    }

    #[test]
    fn debug_redacts_password() {
        let debug = format!("{:?}", connection());
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret"));
    }
}
