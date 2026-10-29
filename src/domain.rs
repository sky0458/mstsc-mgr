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
    /// Resolve this credential at launch time instead of copying its password into the connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<u64>,
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
            .field("credential_id", &self.credential_id)
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

    pub fn session_title(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            self.endpoint()
        } else {
            format!("{name} ({})", self.endpoint())
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedCredential {
    pub id: u64,
    pub name: String,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for SharedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedCredential")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
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
    #[serde(default = "default_floating_opacity_percent")]
    pub floating_opacity_percent: u8,
    #[serde(default)]
    pub floating_ball_x: Option<i32>,
    #[serde(default)]
    pub floating_ball_y: Option<i32>,
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

impl AppSettings {
    pub fn floating_opacity(&self) -> f32 {
        f32::from(self.floating_opacity_percent.clamp(10, 100)) / 100.0
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            floating_controller: true,
            floating_opacity_percent: default_floating_opacity_percent(),
            floating_ball_x: None,
            floating_ball_y: None,
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

const fn default_floating_opacity_percent() -> u8 {
    50
}

const fn default_keepalive_seconds() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultPayload {
    pub format_version: u32,
    #[serde(default)]
    pub connections: Vec<SavedConnection>,
    #[serde(default)]
    pub credentials: Vec<SharedCredential>,
}

impl Default for VaultPayload {
    fn default() -> Self {
        Self {
            format_version: 2,
            connections: Vec::new(),
            credentials: Vec::new(),
        }
    }
}

impl VaultPayload {
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::{bail, ensure};
        ensure!(
            matches!(self.format_version, 1 | 2),
            "unsupported vault format version {}",
            self.format_version
        );
        let mut ids = std::collections::HashSet::new();
        for credential in &self.credentials {
            ensure!(ids.insert(credential.id), "duplicate shared credential ID");
            ensure!(
                !credential.name.trim().is_empty(),
                "credential name is required"
            );
            ensure!(
                !credential.username.trim().is_empty(),
                "credential username is required"
            );
        }
        let mut connection_ids = std::collections::HashSet::new();
        for connection in &self.connections {
            ensure!(
                connection_ids.insert(connection.id),
                "duplicate connection ID"
            );
            if let Some(id) = connection.credential_id
                && !ids.contains(&id)
            {
                bail!(
                    "connection {} references a missing shared credential",
                    connection.name
                );
            }
        }
        Ok(())
    }

    pub fn resolve_connection(&self, id: u64) -> anyhow::Result<SavedConnection> {
        use anyhow::Context as _;
        let mut connection = self
            .connections
            .iter()
            .find(|item| item.id == id)
            .cloned()
            .context("connection no longer exists")?;
        if let Some(credential_id) = connection.credential_id {
            let credential = self.credentials.iter().find(|item| item.id == credential_id)
                .context("shared credential no longer exists; select a valid credential before connecting")?;
            connection.username.clone_from(&credential.username);
            connection.password.clone_from(&credential.password);
        }
        Ok(connection)
    }

    pub fn next_credential_id(&self) -> u64 {
        self.credentials
            .iter()
            .map(|item| item.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn remove_credential(&mut self, id: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self
                .connections
                .iter()
                .any(|item| item.credential_id == Some(id)),
            "credential is still used by a connection; change its credential selection first"
        );
        self.credentials.retain(|item| item.id != id);
        Ok(())
    }

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
            credential_id: None,
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

    #[test]
    fn shared_password_changes_apply_to_all_linked_connections() -> anyhow::Result<()> {
        let mut first = connection();
        first.credential_id = Some(10);
        let mut second = first.clone();
        second.id = 2;
        second.host = "other-host".into();
        let mut vault = VaultPayload {
            connections: vec![first, second],
            credentials: vec![SharedCredential {
                id: 10,
                name: "Shared".into(),
                username: "DOMAIN\\shared".into(),
                password: "before".into(),
            }],
            ..Default::default()
        };
        vault.validate()?;
        assert_eq!(vault.resolve_connection(1)?.password, "before");
        vault.credentials[0].password = "after".into();
        for id in [1, 2] {
            let resolved = vault.resolve_connection(id)?;
            assert_eq!(resolved.password, "after");
            assert_eq!(resolved.username, "DOMAIN\\shared");
        }
        assert!(vault.remove_credential(10).is_err());
        vault.connections[0].credential_id = None;
        assert_eq!(vault.resolve_connection(1)?.password, "secret");
        assert!(vault.remove_credential(10).is_err());
        vault.connections[1].credential_id = None;
        vault.remove_credential(10)?;
        Ok(())
    }

    #[test]
    fn legacy_vault_deserializes_without_shared_accounts() -> anyhow::Result<()> {
        let mut value = serde_json::to_value(connection())?;
        value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("connection should serialize as an object"))?
            .remove("credential_id");
        let vault: VaultPayload = serde_json::from_value(
            serde_json::json!({ "format_version": 1, "connections": [value] }),
        )?;
        vault.validate()?;
        assert!(vault.credentials.is_empty());
        assert_eq!(vault.resolve_connection(1)?.password, "secret");
        Ok(())
    }

    #[test]
    fn missing_reference_fails_without_using_stale_inline_password() {
        let mut profile = connection();
        profile.credential_id = Some(99);
        let vault = VaultPayload {
            connections: vec![profile],
            ..Default::default()
        };
        assert!(vault.validate().is_err());
        assert!(vault.resolve_connection(1).is_err());
    }

    #[test]
    fn shared_accounts_survive_export_payload_roundtrip_and_redact_debug() -> anyhow::Result<()> {
        let mut profile = connection();
        profile.credential_id = Some(1);
        profile.username.clear();
        profile.password.clear();
        let vault = VaultPayload {
            connections: vec![profile],
            credentials: vec![SharedCredential {
                id: 1,
                name: "Shared".into(),
                username: "user".into(),
                password: "vault-password".into(),
            }],
            ..Default::default()
        };
        let restored: VaultPayload = serde_json::from_slice(&serde_json::to_vec(&vault)?)?;
        restored.validate()?;
        assert_eq!(restored.resolve_connection(1)?.password, "vault-password");
        assert!(restored.connections[0].password.is_empty());
        assert!(!format!("{restored:?}").contains("vault-password"));
        Ok(())
    }

    #[test]
    fn session_name_contains_saved_name_host_and_custom_port() {
        let mut profile = connection();
        assert_eq!(profile.session_title(), "dev (10.0.0.8)");
        profile.port = 3390;
        profile.name = "生产数据库".into();
        assert_eq!(profile.session_title(), "生产数据库 (10.0.0.8:3390)");
    }

    #[test]
    fn floating_opacity_defaults_to_half() {
        let settings = AppSettings::default();
        assert_eq!(settings.floating_opacity_percent, 50);
        assert!((settings.floating_opacity() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn floating_position_defaults_to_unspecified() {
        let settings = AppSettings::default();
        assert_eq!(settings.floating_ball_x, None);
        assert_eq!(settings.floating_ball_y, None);
    }
}
