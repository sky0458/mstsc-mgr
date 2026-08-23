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
- Before launch, the selected account is written to the Windows credential target `TERMSRV/<host>` and MSTSC is started with `/v:<host[:port]>`.
- Optional `/f` full-screen launch.

There is intentionally no floating controller, tray integration, MSTSC window discovery, embedded RDP, tabs, keepalive, hotkeys, session switching, GPUI compatibility layer, or WebView technology.

## Data and security

Profiles are stored at:

```text
%LOCALAPPDATA%\mstsc-mgr-external\connections.json
```

Passwords are never stored as plaintext. They are encrypted using Windows DPAPI (`CryptProtectData`) and can only be decrypted by the same Windows user profile on the same Windows installation context. When a connection is launched, the decrypted password is written to Windows Credential Manager only for the `TERMSRV/<host>` target used by MSTSC.

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
