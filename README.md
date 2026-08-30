# mstsc-mgr external (Win2016 native branch)

This branch is a deliberately small, independent Rust/Win32 MSTSC account manager for older Windows systems.

## Target systems

- Windows Server 2016 x64
- Windows 10 1607 x64 and later

The UI is built directly with Win32 controls through `windows-rs`. It does **not** use GPUI, WebView/WebView2, WGPU, DirectX rendering, DirectComposition, Electron, or a browser runtime.

## Scope

Only external MSTSC account management is included:

- Save connection name, host/IP, port and username.
- Save passwords encrypted with the current Windows user's DPAPI key.
- Add, edit and delete saved connections.
- Double-click a connection or press **Connect** to launch the system `mstsc.exe`.
- A per-connection temporary `.rdp` file is generated for every launch.
- The saved password is decrypted only at launch time, then re-encrypted with Windows DPAPI over UTF-16LE bytes and embedded as the standard `password 51:b:<DPAPI hex>` RDP setting. The password is never written as plaintext.
- Domain-style usernames such as `DOMAIN\\user` are written as separate `domain:s:DOMAIN` and `username:s:user` settings for compatibility with older MSTSC/CredSSP behavior.
- The generated RDP profile explicitly sets `prompt for credentials:i:0`, `authentication level:i:0`, `enablecredsspsupport:i:1`, `promptcredentialonce:i:1`, `negotiate security layer:i:1` and `public mode:i:0`.
- `authentication level:i:0` corresponds to MSTSC's **Connect and don't warn me** server-authentication behavior. This is enabled for compatibility with older/internal RDP endpoints and means server identity warnings are not enforced by this profile.
- CredSSP is explicitly enabled, matching the compatibility option commonly required by Windows Server / RDO-managed connections.
- Optional full-screen launch.

There is intentionally no floating controller, tray integration, MSTSC window discovery, embedded RDP, tabs, keepalive, hotkeys, session switching, GPUI compatibility layer, or WebView technology.

## Data and security

Profiles are stored at:

```text
%LOCALAPPDATA%\mstsc-mgr-external\connections.json
```

Passwords are stored in the profile only as DPAPI-protected Base64 blobs. On launch, the application decrypts the saved password in memory and immediately creates the MSTSC-compatible `password 51:b:` DPAPI blob for the generated `.rdp` file. That RDP password blob is bound to the current Windows user/machine context and cannot be used as plaintext.

The application no longer relies on a `TERMSRV/<host>` Credential Manager entry as the primary password path. This avoids stale or conflicting saved credentials taking precedence over the selected profile.

If a domain policy explicitly forces **Always prompt for password upon connection** or blocks credential delegation, Windows policy can still override client-side RDP settings.

## Build

```powershell
cargo build --release
```

Output:

```text
target\release\mstsc-mgr-external.exe
```

## CI compatibility guard

The branch CI rejects release binaries that import the graphics/browser compatibility problems this branch is designed to avoid:

- `d3d11.dll`
- `dxgi.dll`
- `dcomp.dll`
- `icuuc.dll`
- `WebView2Loader.dll`

The build artifact is named `mstsc-mgr-external-win2016-x64`.
