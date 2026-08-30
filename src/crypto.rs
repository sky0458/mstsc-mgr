use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::fmt::Write as _;
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::PWSTR,
};

pub fn protect_text(cleartext: &str) -> Result<String> {
    if cleartext.is_empty() {
        return Ok(String::new());
    }
    let encrypted = protect(cleartext.as_bytes())?;
    Ok(STANDARD.encode(encrypted))
}

pub fn unprotect_text(encoded: &str) -> Result<String> {
    if encoded.is_empty() {
        return Ok(String::new());
    }
    let encrypted = STANDARD
        .decode(encoded)
        .context("stored password is not valid base64")?;
    let cleartext = unprotect(&encrypted)?;
    String::from_utf8(cleartext).context("decrypted password is not valid UTF-8")
}

pub fn protect_rdp_password(cleartext: &str) -> Result<String> {
    if cleartext.is_empty() {
        return Ok(String::new());
    }

    // The mstsc `password 51:b:` field is a DPAPI blob over UTF-16LE password bytes.
    // It is bound to the current Windows user/machine context and is not plaintext.
    let mut utf16le = Vec::with_capacity(cleartext.encode_utf16().count() * 2);
    for unit in cleartext.encode_utf16() {
        utf16le.extend_from_slice(&unit.to_le_bytes());
    }
    let encrypted = protect(&utf16le)?;
    let mut hex = String::with_capacity(encrypted.len() * 2);
    for byte in encrypted {
        write!(&mut hex, "{byte:02X}").context("failed to encode RDP password")?;
    }
    Ok(hex)
}

fn protect(cleartext: &[u8]) -> Result<Vec<u8>> {
    if cleartext.is_empty() {
        return Ok(Vec::new());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(cleartext.len()).context("password is too large")?,
        pbData: cleartext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .context("CryptProtectData failed")?;
    }
    copy_and_free_blob(output)
}

fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.is_empty() {
        return Ok(Vec::new());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(ciphertext.len()).context("encrypted password is too large")?,
        pbData: ciphertext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = PWSTR::null();
    unsafe {
        CryptUnprotectData(
            &input,
            Some(&mut description),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .context("CryptUnprotectData failed")?;
        if !description.is_null() {
            let _ = LocalFree(Some(HLOCAL(description.0.cast())));
        }
    }
    copy_and_free_blob(output)
}

fn copy_and_free_blob(blob: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>> {
    if blob.pbData.is_null() || blob.cbData == 0 {
        bail!("DPAPI returned an empty output blob");
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdp_password_uses_dpapi_over_utf16le() {
        let password = "P@ssw0rd-测试";
        let hex = protect_rdp_password(password).expect("protect RDP password");
        assert!(!hex.is_empty());
        assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(hex.len() % 2, 0);

        let encrypted = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex byte"))
            .collect::<Vec<_>>();
        let clear = unprotect(&encrypted).expect("unprotect RDP password");
        assert_eq!(clear.len() % 2, 0);
        let units = clear
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        assert_eq!(String::from_utf16(&units).expect("UTF-16 password"), password);
    }
}
