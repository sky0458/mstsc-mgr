use anyhow::{Context, Result, bail};
use windows::{
    Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::PWSTR,
};

pub fn protect(cleartext: &[u8]) -> Result<Vec<u8>> {
    if cleartext.is_empty() {
        bail!("refusing to encrypt an empty payload");
    }

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(cleartext.len()).context("payload too large")?,
        pbData: cleartext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    // SAFETY: input points to `cleartext` for the duration of the call. Optional pointers are
    // null. Windows allocates output with LocalAlloc and we copy it before LocalFree.
    unsafe {
        CryptProtectData(
            &mut input,
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

pub fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.is_empty() {
        bail!("encrypted payload is empty");
    }

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(ciphertext.len()).context("payload too large")?,
        pbData: ciphertext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = PWSTR::null();

    // SAFETY: input points to `ciphertext` for the duration of the call. Output and optional
    // description are allocated by Windows and released below with LocalFree.
    unsafe {
        CryptUnprotectData(
            &mut input,
            Some(&mut description),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .context("CryptUnprotectData failed")?;
        if !description.is_null() {
            let _ = LocalFree(Some(description.0.cast()));
        }
    }

    copy_and_free_blob(output)
}

fn copy_and_free_blob(blob: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>> {
    if blob.pbData.is_null() || blob.cbData == 0 {
        bail!("DPAPI returned an empty output blob");
    }

    // SAFETY: DPAPI returned a valid allocation of `cbData` bytes in `pbData`. We copy before
    // freeing with LocalFree, as required by the API contract.
    unsafe {
        let slice = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize);
        let bytes = slice.to_vec();
        let _ = LocalFree(Some(blob.pbData.cast::<core::ffi::c_void>()));
        Ok(bytes)
    }
}
