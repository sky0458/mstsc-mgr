# mstsc-mgr

A Windows 10+ native MSTSC manager written in **Rust + GPUI**, modeled after the useful parts of RDM's **External** mode: the RDP client remains Microsoft's `mstsc.exe`, while mstsc-mgr owns saved connections, secure credentials, global switching and a floating session controller.

## Features

- Native Rust/GPUI desktop app; no .NET/WPF/WinUI, Electron, Java, Python or Node runtime.
- Save multiple RDP connections with host, port, username, password and optional MSTSC arguments.
- Passwords and the local vault are encrypted with Windows DPAPI; plaintext secrets are not written to disk.
- Launches external `mstsc.exe` and writes `TERMSRV/<host>` credentials using Windows Credential Manager instead of putting passwords on the command line.
- Settings dialog for floating controller, persistent tabs, global hotkeys and keepalive behavior.
- Encrypted vault import/export. **v0.1 exports are DPAPI current-user bound**, so importing requires the same Windows user profile.
- Topmost transparent floating RDP controller. Hover expands the current MSTSC processes/windows; always-visible mode keeps translucent vertical tabs below it.
- Click a floating tab to restore/activate its MSTSC window.
- Global switching:
  - `Alt+Shift+1..9`: activate the Nth current visible MSTSC window.
  - `Ctrl+Alt+Shift+Left/Right`: cycle through current MSTSC windows with wrap-around.
- Optional keepalive with a targeted `WM_MOUSEMOVE` or Shift key message sent only to enumerated MSTSC HWNDs. It does not move the physical mouse or steal foreground focus.
- GitHub Actions CI on Windows and tagged release packaging.

## Security model

Local data is stored under `%LOCALAPPDATA%\mstsc-mgr`:

- `settings.json`: non-secret settings.
- `vault.dpapi`: DPAPI-encrypted serialized connection vault.

When connecting, the password is copied into Windows Credential Manager for target `TERMSRV/<host>` through `CredWriteW`. Only `/v:<host[:port]>` and optional user-supplied MSTSC switches are passed to `mstsc.exe`.

## Development

See [`AGENTS.md`](AGENTS.md) before editing. The file defines module boundaries, Windows API constraints, secret-handling requirements and the mandatory README version log.

Quality gates used by CI:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

## Release

Push a SemVer tag matching `Cargo.toml`, for example `v0.1.1`. `.github/workflows/release.yml` builds on `windows-2025`, packages `mstsc-mgr.exe`, README and LICENSE into `mstsc-mgr-windows-x64.zip`, and publishes it as a GitHub Release asset.

## Development Log

### version 0.1.1 2026-08-22 16:51:55

- Applied the first `rustfmt` normalization reported by Windows CI.
- Temporarily made the format step non-blocking on the feature branch so Clippy/test/release-build can expose the actual GPUI/Win32 API compatibility errors in the same CI pass; strict formatting is restored before the PR is marked ready.

### version 0.1.0 2026-08-22 16:38:00

- Bootstrapped the Windows-only Rust 2024 + GPUI 0.2.2 application and documented strict architecture/code rules in `AGENTS.md`.
- Added a full saved-connection manager UI with add/edit/delete/connect actions, password masking and a settings dialog.
- Added DPAPI local vault encryption plus encrypted import/export, with explicit same-Windows-user portability semantics for v0.1.
- Added Windows Credential Manager integration and external `mstsc.exe` launching without password command-line exposure.
- Added Win32 MSTSC process/window discovery, restore/foreground activation, floating hover controller and optional always-visible vertical tabs.
- Added `Alt+Shift+1..9` and `Ctrl+Alt+Shift+Left/Right` global hotkeys via `RegisterHotKey`.
- Added targeted, configurable keepalive messages that avoid moving the physical pointer or focusing a session.
- Added Windows GitHub Actions for formatting, Clippy, tests, release build, build artifact upload and SemVer-tagged GitHub Releases.
