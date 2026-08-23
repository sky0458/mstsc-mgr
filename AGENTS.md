# AGENTS.md

This branch is a deliberately independent product line for legacy Windows.

## Product boundary

- Branch: `external/server2016`.
- Product: `mstsc-mgr-external`, a minimal external MSTSC credential manager.
- OS target: Windows Server 2016 / Windows 10 1607 and newer, x64.
- Language/UI: Rust + native Win32 controls through the `windows` crate only.
- Do not use GPUI, WebView, Electron, .NET/WPF/WinUI, Direct3D, DirectComposition, or any browser runtime.
- MSTSC remains a normal external `mstsc.exe` process/window.

## Only supported product features

- List saved RDP accounts.
- Add, edit, and delete accounts.
- Connect by writing a `TERMSRV/<host>` credential through `CredWriteW` and launching `mstsc.exe /v:<host>`.
- Passwords are stored only as current-user DPAPI ciphertext in the local JSON configuration.
- Editing an existing entry with an empty password field preserves the current saved password.

Do not add floating UI, session discovery, hotkeys, keepalive, tray behavior, import/export, WebView, embedded RDP, or background services to this branch.

## Secret handling

- Never store plaintext passwords in the JSON file.
- Never put usernames or passwords on the `mstsc.exe` command line.
- Never log passwords or decrypted DPAPI material.
- Password plaintext may exist only briefly in process memory while updating Windows Credential Manager.

## Compatibility constraints

- Use only Win32 APIs available on Windows Server 2016 / Windows 10 1607.
- Release builds statically link the MSVC CRT to avoid requiring a separately installed VC++ runtime.
- CI must reject accidental imports of `d3d11.dll`, `dxgi.dll`, `dcomp.dll`, `icuuc.dll`, or WebView runtimes.

## Code quality

- Every `unsafe` block must have a nearby `// SAFETY:` comment.
- No `unwrap()` / `expect()` in production code.
- Run before shipping:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `cargo build --release`

## Version log

Every coding change must update README.md under `Development Log` with Asia/Taipei local time using:

`version x.y.z yyyy-MM-dd hh:mm:ss`
