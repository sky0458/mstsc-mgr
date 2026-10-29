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
- The first saved connection is selected automatically when the app starts or the list is refreshed.
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


## Shared accounts

Use **Shared accounts** to switch the list to saved accounts. **Add**, **Edit** and **Delete** operate on the displayed list. Create one shared account, return to **Connections**, and select it in each connection's **Account** dropdown. Updating its username/password once applies to all linked connections on their next launch. Connections can still use an individual account. Linking removes the connection's individual credential copy; blank passwords in the editor keep the existing saved password. Referenced accounts cannot be deleted.

Shared passwords remain DPAPI-protected Base64 blobs in `connections.json`; they are decrypted only at launch. Existing version 0/1 files load and are saved as version 2. Back up the data before upgrading and use version 0.2.0 or newer to edit files containing shared accounts. The GPUI branch has a different data format and location; the branches do not exchange vault files.

The connection editor is modal: its owner is disabled until the editor closes, capture is released on entry/exit, and native dialog keyboard navigation is supported. The native title bar retains normal Windows dragging. This branch has no floating controller, so the GPUI floating-icon fix does not apply here.

MSTSC main windows launched by this tool receive best-effort `saved name (host[:port])` titles. The title monitor stops when the launched process exits, does not touch other sessions or credential dialogs, and uses bounded messages to avoid waiting indefinitely on hung clients. Elevated clients or process handoff may prevent renaming; the fullscreen connection bar may still display MSTSC's own label.

## Development Log

### version 0.2.0 2026-10-29 22:43:32

- Added named shared accounts with DPAPI-protected passwords and connection references; account updates apply to every linked connection on its next launch. Preserved reading existing data and individual connection credentials, and blocked deletion of referenced accounts.
- Added shared-account management and selection using standard Win32 controls, preserving the Server 2016 API/runtime dependency boundary.
- Fixed editor ownership, capture cleanup, keyboard navigation, message-loop shutdown and data lifetime so competing editors cannot interfere with native dialog dragging. Failed saves do not replace the in-memory store.
- Added best-effort saved-name/host titles for only tool-launched MSTSC main windows, with process lifetime tracking and bounded window messages.
