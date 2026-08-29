use crate::domain::SavedConnection;
use anyhow::{Context, Result, bail};
use std::{fs, mem::size_of, path::PathBuf, process::Command};
use windows::{
    Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_DOMAIN_PASSWORD, CRED_TYPE_GENERIC, CREDENTIALW,
        CredDeleteW, CredWriteW,
    },
    core::{PCWSTR, PWSTR},
};

pub fn launch_connection(connection: &SavedConnection) -> Result<()> {
    if connection.host.trim().is_empty() {
        bail!("host is required");
    }

    let has_saved_credentials =
        !connection.username.trim().is_empty() && !connection.password.is_empty();
    tracing::info!(
        host = %connection.host,
        port = connection.port,
        has_saved_credentials,
        "launching MSTSC connection with dedicated RDP profile"
    );

    if has_saved_credentials {
        refresh_rdp_credential(connection)?;
    }

    let rdp_path = write_rdp_file(connection, has_saved_credentials)?;
    let mut command = Command::new("mstsc.exe");
    command.arg(&rdp_path);
    for arg in sanitized_mstsc_args(connection, has_saved_credentials) {
        command.arg(arg);
    }
    command
        .spawn()
        .with_context(|| format!("failed to launch mstsc.exe with {}", rdp_path.display()))?;
    Ok(())
}

fn refresh_rdp_credential(connection: &SavedConnection) -> Result<()> {
    let target = wide_null(&connection.credential_target());
    let username = wide_null(&connection.username);
    let password: Vec<u16> = connection.password.encode_utf16().collect();
    let password_bytes = password
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|len| u32::try_from(len).ok())
        .context("password is too large")?;

    // Clear both credential types before writing the current Generic TERMSRV credential. A stale
    // Domain Password entry for the same server can otherwise take precedence and make mstsc.exe
    // prompt again even though mstsc-mgr has a current saved password.
    unsafe {
        let target_name = PCWSTR(target.as_ptr());
        let _ = CredDeleteW(target_name, CRED_TYPE_GENERIC, None);
        let _ = CredDeleteW(target_name, CRED_TYPE_DOMAIN_PASSWORD, None);
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

    // SAFETY: all CREDENTIALW pointers refer to buffers alive for this synchronous CredWriteW call.
    unsafe { CredWriteW(&credential, 0).context("CredWriteW failed")? };
    Ok(())
}

fn write_rdp_file(connection: &SavedConnection, has_saved_credentials: bool) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("mstsc-mgr-connection-{}.rdp", connection.id));
    let content = build_rdp_content(connection, has_saved_credentials);
    let mut bytes = Vec::with_capacity(content.len().saturating_mul(2).saturating_add(2));
    bytes.extend_from_slice(&[0xff, 0xfe]);
    for unit in content.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn build_rdp_content(connection: &SavedConnection, has_saved_credentials: bool) -> String {
    let prompt_for_credentials = if has_saved_credentials { 0 } else { 1 };
    format!(
        concat!(
            "full address:s:{}\r\n",
            "username:s:{}\r\n",
            "prompt for credentials:i:{}\r\n",
            "promptcredentialonce:i:1\r\n",
            "enablecredsspsupport:i:1\r\n",
            "negotiate security layer:i:1\r\n",
            "autoreconnection enabled:i:1\r\n"
        ),
        connection.endpoint(),
        connection.username,
        prompt_for_credentials,
    )
}

fn sanitized_mstsc_args(connection: &SavedConnection, has_saved_credentials: bool) -> Vec<String> {
    connection
        .mstsc_args
        .iter()
        .filter_map(|arg| {
            let trimmed = arg.trim();
            if trimmed.is_empty() || trimmed.to_ascii_lowercase().starts_with("/v:") {
                return None;
            }
            if has_saved_credentials && trimmed.eq_ignore_ascii_case("/prompt") {
                return None;
            }
            Some(trimmed.to_string())
        })
        .collect()
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> SavedConnection {
        SavedConnection {
            id: 7,
            name: "test".to_string(),
            host: "10.0.0.8".to_string(),
            port: 3390,
            username: "DOMAIN\\user".to_string(),
            password: "must-not-be-written".to_string(),
            mstsc_args: vec![
                "/f".to_string(),
                "/v:wrong-host".to_string(),
                "/prompt".to_string(),
            ],
        }
    }

    #[test]
    fn rdp_content_uses_saved_credentials_without_embedding_password() {
        let profile = connection();
        let content = build_rdp_content(&profile, true);
        assert!(content.contains("full address:s:10.0.0.8:3390"));
        assert!(content.contains("username:s:DOMAIN\\user"));
        assert!(content.contains("prompt for credentials:i:0"));
        assert!(content.contains("promptcredentialonce:i:1"));
        assert!(content.contains("enablecredsspsupport:i:1"));
        assert!(!content.contains("must-not-be-written"));
    }

    #[test]
    fn rdp_content_keeps_prompt_for_profiles_without_saved_password() {
        let mut profile = connection();
        profile.password.clear();
        let content = build_rdp_content(&profile, false);
        assert!(content.contains("prompt for credentials:i:1"));
    }

    #[test]
    fn conflicting_command_line_args_are_removed() {
        let profile = connection();
        let args = sanitized_mstsc_args(&profile, true);
        assert_eq!(args, vec!["/f"]);
    }
}
