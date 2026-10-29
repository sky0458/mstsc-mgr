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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<u64>,
    #[serde(default)]
    pub fullscreen: bool,
}

impl ConnectionProfile {
    pub fn session_title(&self) -> String {
        if self.name.trim().is_empty() {
            self.endpoint()
        } else {
            format!("{} ({})", self.name.trim(), self.endpoint())
        }
    }
    pub fn endpoint(&self) -> String {
        if self.port == default_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedCredential {
    pub id: u64,
    pub name: String,
    pub username: String,
    pub protected_password: String,
}

impl std::fmt::Debug for SharedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedCredential")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("username", &self.username)
            .field("protected_password", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStore {
    #[serde(default = "format_version")]
    pub format_version: u32,
    #[serde(default)]
    pub connections: Vec<ConnectionProfile>,
    #[serde(default)]
    pub credentials: Vec<SharedCredential>,
}

impl Default for ConnectionStore {
    fn default() -> Self {
        Self {
            format_version: 2,
            connections: Vec::new(),
            credentials: Vec::new(),
        }
    }
}

impl ConnectionStore {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(self.format_version, 0..=2),
            "unsupported connection store format version {}",
            self.format_version
        );
        let mut ids = std::collections::HashSet::new();
        for item in &self.credentials {
            anyhow::ensure!(ids.insert(item.id), "duplicate shared account ID");
            anyhow::ensure!(!item.name.trim().is_empty(), "account name is required");
            anyhow::ensure!(
                !item.username.trim().is_empty(),
                "account username is required"
            );
        }
        let mut connection_ids = std::collections::HashSet::new();
        for item in &self.connections {
            anyhow::ensure!(connection_ids.insert(item.id), "duplicate connection ID");
            if let Some(id) = item.credential_id {
                anyhow::ensure!(
                    ids.contains(&id),
                    "connection {} references a missing shared account",
                    item.name
                );
            }
        }
        Ok(())
    }

    pub fn resolve_profile(&self, id: u64) -> anyhow::Result<ConnectionProfile> {
        use anyhow::Context as _;
        let mut profile = self
            .connections
            .iter()
            .find(|item| item.id == id)
            .cloned()
            .context("connection no longer exists")?;
        if let Some(id) = profile.credential_id {
            let account = self.credentials.iter().find(|item| item.id == id).context(
                "shared account no longer exists; select a valid account before connecting",
            )?;
            profile.username.clone_from(&account.username);
            profile
                .protected_password
                .clone_from(&account.protected_password);
        }
        Ok(profile)
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
            "account is still used by a connection; change its account selection first"
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

const fn default_port() -> u16 {
    3389
}

const fn format_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: u64, credential_id: Option<u64>) -> ConnectionProfile {
        ConnectionProfile {
            id,
            name: "生产主机".into(),
            host: "10.0.0.8".into(),
            port: 3390,
            username: "local-user".into(),
            protected_password: "old-protected-blob".into(),
            credential_id,
            fullscreen: false,
        }
    }

    #[test]
    fn shared_password_rotation_reaches_every_linked_connection() -> anyhow::Result<()> {
        let mut store = ConnectionStore {
            connections: vec![profile(1, Some(7)), profile(2, Some(7))],
            credentials: vec![SharedCredential {
                id: 7,
                name: "Common".into(),
                username: "DOMAIN\\shared".into(),
                protected_password: "before".into(),
            }],
            ..Default::default()
        };
        store.validate()?;
        store.credentials[0].protected_password = "after".into();
        for id in [1, 2] {
            let resolved = store.resolve_profile(id)?;
            assert_eq!(resolved.username, "DOMAIN\\shared");
            assert_eq!(resolved.protected_password, "after");
        }
        assert!(store.remove_credential(7).is_err());
        store.connections[0].credential_id = None;
        assert_eq!(
            store.resolve_profile(1)?.protected_password,
            "old-protected-blob"
        );
        store.connections[1].credential_id = None;
        store.remove_credential(7)?;
        assert!(store.credentials.is_empty());
        assert_eq!(store.next_id(), 3);
        assert_eq!(store.next_credential_id(), 1);
        Ok(())
    }

    #[test]
    fn legacy_files_without_shared_accounts_load() -> anyhow::Result<()> {
        for version in [0, 1] {
            let store: ConnectionStore = serde_json::from_value(serde_json::json!({
                "format_version": version,
                "connections": [{ "id": 1, "name": "Old", "host": "host", "username": "user", "protected_password": "blob" }]
            }))?;
            store.validate()?;
            assert!(store.credentials.is_empty());
            assert_eq!(store.resolve_profile(1)?.protected_password, "blob");
        }
        Ok(())
    }

    #[test]
    fn invalid_reference_does_not_fall_back_to_old_password() {
        let store = ConnectionStore {
            connections: vec![profile(1, Some(99))],
            ..Default::default()
        };
        assert!(store.validate().is_err());
        assert!(store.resolve_profile(1).is_err());
    }

    #[test]
    fn shared_accounts_and_references_roundtrip_without_plaintext_passwords() -> anyhow::Result<()>
    {
        let mut connection = profile(1, Some(7));
        connection.protected_password.clear();
        connection.username.clear();
        let store = ConnectionStore {
            connections: vec![connection],
            credentials: vec![SharedCredential {
                id: 7,
                name: "Shared".into(),
                username: "user".into(),
                protected_password: "DPAPI-base64-placeholder".into(),
            }],
            ..Default::default()
        };
        let restored: ConnectionStore = serde_json::from_slice(&serde_json::to_vec(&store)?)?;
        restored.validate()?;
        assert!(restored.connections[0].protected_password.is_empty());
        assert_eq!(
            restored.resolve_profile(1)?.protected_password,
            "DPAPI-base64-placeholder"
        );
        assert!(!format!("{:?}", restored.credentials).contains("DPAPI-base64-placeholder"));
        Ok(())
    }

    #[test]
    fn session_name_keeps_unicode_name_host_and_nondefault_port() {
        let mut connection = profile(1, None);
        assert_eq!(connection.session_title(), "生产主机 (10.0.0.8:3390)");
        connection.name.clear();
        connection.port = 3389;
        assert_eq!(connection.session_title(), "10.0.0.8");
    }
}
