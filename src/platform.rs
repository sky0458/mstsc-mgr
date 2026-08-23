use crate::{crypto, model::SavedConnection};
use anyhow::{Context, Result, bail};
use std::{mem::size_of, process::Command};
use windows::{
    Win32::Security::Credentials::{
        CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredWriteW,
    },
    core::{PCWSTR, PWSTR},
};

pub fn connect(connection: &SavedConnection) -> Result<()> {
    let host = connection.host.trim();
    if host.is_empty() {
        bail!("IP / 主机名不能为空");
    }

    let password = crypto::unprotect_password(&connection.password_dpapi)?;
    write_rdp_credential(host, connection.username.trim(), &password)?;

    Command::new("mstsc.exe")
        .arg(format!("/v:{host}"))
        .spawn()
        .context("启动 mstsc.exe 失败")?;
    Ok(())
}

pub fn delete_credential(host: &str) {
    let host = host.trim();
    if host.is_empty() {
        return;
    }
    let target = wide_null(&credential_target(host));

    // SAFETY: target is a valid NUL-terminated UTF-16 string for the duration of the synchronous
    // CredDeleteW call. Failure is intentionally ignored because the credential may not exist yet.
    unsafe {
        let _ = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0);
    }
}

fn write_rdp_credential(host: &str, username: &str, password: &str) -> Result<()> {
    if username.is_empty() {
        bail!("用户名不能为空");
    }
    if password.is_empty() {
        bail!("密码不能为空");
    }

    let target = wide_null(&credential_target(host));
    let username = wide_null(username);
    let password_utf16: Vec<u16> = password.encode_utf16().collect();
    let password_bytes = password_utf16
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|len| u32::try_from(len).ok())
        .context("密码过长")?;

    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr().cast_mut()),
        CredentialBlobSize: password_bytes,
        CredentialBlob: password_utf16.as_ptr().cast::<u8>().cast_mut(),
        Persist: CRED_PERSIST_ENTERPRISE,
        UserName: PWSTR(username.as_ptr().cast_mut()),
        ..Default::default()
    };

    // SAFETY: all pointers in CREDENTIALW refer to UTF-16 buffers that remain alive for the
    // synchronous CredWriteW call. CredentialBlobSize is expressed in bytes as required by Win32.
    unsafe {
        CredWriteW(&credential, 0).context("写入 Windows Credential Manager 失败")?;
    }
    Ok(())
}

fn credential_target(host: &str) -> String {
    format!("TERMSRV/{host}")
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
