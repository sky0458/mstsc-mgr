use crate::{crypto, model::ConnectionProfile};
use anyhow::{Context, Result, bail};
use std::{fs, mem::size_of, path::PathBuf, process::Command};
use windows::{
    Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_DOMAIN_PASSWORD, CRED_TYPE_GENERIC, CREDENTIALW,
        CredDeleteW, CredWriteW,
    },
    core::{PCWSTR, PWSTR},
};

pub fn launch(profile: &ConnectionProfile) -> Result<()> {
    if profile.host.trim().is_empty() {
        bail!("host is required");
    }

    if !profile.username.trim().is_empty() && !profile.protected_password.is_empty() {
        let password = crypto::unprotect_text(&profile.protected_password)?;
        if !password.is_empty() {
            write_rdp_credential(profile, &password)?;
        }
    }

    let rdp_path = write_rdp_file(profile)?;
    Command::new("mstsc.exe")
        .arg(&rdp_path)
        .spawn()
        .with_context(|| format!("failed to launch mstsc.exe with {}", rdp_path.display()))?;
    Ok(())
}

fn write_rdp_credential(profile: &ConnectionProfile, password: &str) -> Result<()> {
    let target = wide_null(&profile.credential_target());
    let username = wide_null(&profile.username);
    let password: Vec<u16> = password.encode_utf16().collect();
    let password_bytes = password
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|len| u32::try_from(len).ok())
        .context("password is too large")?;

    // Remove stale credentials for the same TERMSRV target first. Old Domain Password
    // entries can take precedence over the Generic credential that mstsc can consume.
    unsafe {
        let target_name = PCWSTR(target.as_ptr());
        let _ = CredDeleteW(target_name, CRED_TYPE_GENERIC, 0);
        let _ = CredDeleteW(target_name, CRED_TYPE_DOMAIN_PASSWORD, 0);
    }

    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr().cast_mut()),
        CredentialBlobSize: password_bytes,
        CredentialBlob: password.as_ptr().cast::<u8>().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(username.as_ptr().cast_mut()),
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0).context("CredWriteW failed")? };
    Ok(())
}

fn write_rdp_file(profile: &ConnectionProfile) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("mstsc-mgr-external-{}.rdp", profile.id));
    let content = build_rdp_content(profile);
    let mut bytes = Vec::with_capacity(content.len() * 2 + 2);
    bytes.extend_from_slice(&[0xff, 0xfe]);
    for unit in content.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn build_rdp_content(profile: &ConnectionProfile) -> String {
    let screen_mode = if profile.fullscreen { 2 } else { 1 };
    format!(
        concat!(
            "full address:s:{}\r\n",
            "username:s:{}\r\n",
            "screen mode id:i:{}\r\n",
            "prompt for credentials:i:0\r\n",
            "promptcredentialonce:i:1\r\n",
            "authentication level:i:0\r\n",
            "enablecredsspsupport:i:1\r\n",
            "negotiate security layer:i:1\r\n",
            "autoreconnection enabled:i:1\r\n"
        ),
        profile.endpoint(),
        profile.username,
        screen_mode,
    )
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
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
    fn rdp_content_matches_compatibility_settings() {
        let content = build_rdp_content(&profile());
        assert!(content.contains("full address:s:10.0.0.8:3390"));
        assert!(content.contains("username:s:DOMAIN\\user"));
        assert!(content.contains("authentication level:i:0"));
        assert!(content.contains("enablecredsspsupport:i:1"));
        assert!(content.contains("prompt for credentials:i:0"));
        assert!(content.contains("screen mode id:i:2"));
        assert!(!content.contains("must-not-be-written"));
    }
}
