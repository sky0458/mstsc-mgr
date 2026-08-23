use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
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
