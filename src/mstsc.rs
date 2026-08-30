use crate::{crypto, model::ConnectionProfile};
use anyhow::{Context, Result, bail};
use std::{fs, path::PathBuf, process::Command};

pub fn launch(profile: &ConnectionProfile) -> Result<()> {
    if profile.host.trim().is_empty() {
        bail!("host is required");
    }

    let rdp_password = if profile.protected_password.is_empty() {
        None
    } else {
        let password = crypto::unprotect_text(&profile.protected_password)?;
        if password.is_empty() {
            None
        } else {
            Some(crypto::protect_rdp_password(&password)?)
        }
    };

    let rdp_path = write_rdp_file(profile, rdp_password.as_deref())?;
    Command::new("mstsc.exe")
        .arg(&rdp_path)
        .spawn()
        .with_context(|| format!("failed to launch mstsc.exe with {}", rdp_path.display()))?;
    Ok(())
}

fn write_rdp_file(profile: &ConnectionProfile, rdp_password: Option<&str>) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("mstsc-mgr-external-{}.rdp", profile.id));
    let content = build_rdp_content(profile, rdp_password);
    let mut bytes = Vec::with_capacity(content.len() * 2 + 2);
    bytes.extend_from_slice(&[0xff, 0xfe]);
    for unit in content.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn build_rdp_content(profile: &ConnectionProfile, rdp_password: Option<&str>) -> String {
    let screen_mode = if profile.fullscreen { 2 } else { 1 };
    let (domain, username) = split_domain_username(&profile.username);

    let mut content = format!(
        concat!(
            "full address:s:{}\r\n",
            "username:s:{}\r\n",
            "screen mode id:i:{}\r\n",
            "prompt for credentials:i:0\r\n",
            "promptcredentialonce:i:1\r\n",
            "authentication level:i:0\r\n",
            "enablecredsspsupport:i:1\r\n",
            "negotiate security layer:i:1\r\n",
            "public mode:i:0\r\n",
            "autoreconnection enabled:i:1\r\n"
        ),
        profile.endpoint(),
        username,
        screen_mode,
    );

    if let Some(domain) = domain {
        content.push_str("domain:s:");
        content.push_str(domain);
        content.push_str("\r\n");
    }

    if let Some(password) = rdp_password.filter(|password| !password.is_empty()) {
        content.push_str("password 51:b:");
        content.push_str(password);
        content.push_str("\r\n");
    }

    content
}

fn split_domain_username(username: &str) -> (Option<&str>, &str) {
    if let Some((domain, user)) = username.split_once('\\')
        && !domain.is_empty()
        && !user.is_empty()
    {
        return (Some(domain), user);
    }
    (None, username)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ConnectionProfile {
        ConnectionProfile {
            id: 7,
            name: "test".to_string(),
            host: "10.0.0.8".to_string(),
            port: 3390,
            username: "DOMAIN\\user".to_string(),
            protected_password: "must-not-be-written".to_string(),
            fullscreen: true,
        }
    }

    #[test]
    fn rdp_content_matches_compatibility_settings_and_embeds_password() {
        let content = build_rdp_content(&profile(), Some("010203AABB"));
        assert!(content.contains("full address:s:10.0.0.8:3390"));
        assert!(content.contains("username:s:user"));
        assert!(content.contains("domain:s:DOMAIN"));
        assert!(content.contains("password 51:b:010203AABB"));
        assert!(content.contains("authentication level:i:0"));
        assert!(content.contains("enablecredsspsupport:i:1"));
        assert!(content.contains("prompt for credentials:i:0"));
        assert!(content.contains("public mode:i:0"));
        assert!(content.contains("screen mode id:i:2"));
        assert!(!content.contains("must-not-be-written"));
    }

    #[test]
    fn username_without_domain_is_kept_as_is() {
        let mut profile = profile();
        profile.username = "local-user".to_string();
        let content = build_rdp_content(&profile, None);
        assert!(content.contains("username:s:local-user"));
        assert!(!content.contains("domain:s:"));
        assert!(!content.contains("password 51:b:"));
    }
}
