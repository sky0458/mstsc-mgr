use crate::{crypto, model::ConnectionProfile};
use anyhow::{Context, Result, bail};
use std::{mem::size_of, process::Command};
use windows::{
    Win32::Security::Credentials::{
        CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
    },
    core::PWSTR,
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

    let mut command = Command::new("mstsc.exe");
    command.arg(format!("/v:{}", profile.endpoint()));
    if profile.fullscreen {
        command.arg("/f");
    }
    command.spawn().context("failed to launch mstsc.exe")?;
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

    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr().cast_mut()),
        CredentialBlobSize: password_bytes,
        CredentialBlob: password.as_ptr().cast::<u8>().cast_mut(),
        Persist: CRED_PERSIST_ENTERPRISE,
        UserName: PWSTR(username.as_ptr().cast_mut()),
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0).context("CredWriteW failed")? };
    Ok(())
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
