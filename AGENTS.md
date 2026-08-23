# AGENTS.md

This file is authoritative for all human and AI contributors.

## Product boundary

- Product: `mstsc-mgr`, a Windows 10+ native manager for external `mstsc.exe` sessions, inspired by RDM's **External** mode.
- Language/UI: Rust + GPUI. `gpui-component` is allowed because it is a Rust/GPUI component library, not an external runtime.
- OS: Windows 10 and newer only.
- Runtime rule: do **not** introduce .NET/WPF/WinUI, Electron/WebView, Java, Python, Node.js, AutoHotkey, or a resident helper service.
- Native integration must use Windows APIs through the `windows` crate. Do not shell out to PowerShell, WMI, `cmdkey`, or UI automation when a Win32 API exists.
- MSTSC stays an external top-level process/window. Do not embed, re-parent, hook, inject into, or patch `mstsc.exe`.

## Architecture boundaries

- `domain.rs`: serializable product models only; no Win32 or GPUI calls.
- `config.rs`: application paths, settings/vault persistence, import/export orchestration.
- `crypto.rs`: encryption/decryption only. Local secrets use Windows DPAPI.
- `logging.rs`: diagnostic file writer/bootstrap only. It may consume runtime settings but must never serialize or log secrets.
- `platform.rs`: core Win32 process/window/credential/hotkey/keepalive/floating-window/tray operations shared across the application.
- `platform_actions.rs`: focused user-triggered Win32 window actions layered on top of `platform.rs`, currently Host Desktop switching and main-window native-frame repair. It must reuse the system-wide MSTSC enumeration from `platform.rs` rather than duplicate process-identification logic.
- `floating_position.rs`: Win32 floating-ball startup placement and native drag-position persistence only; it may update non-secret position settings through `config`.
- `floating.rs`: GPUI presentation/state for the independent floating-ball and MSTSC-list popup components; general native HWND work stays in `platform.rs`/`platform_actions.rs`, while startup placement/persistence stays in `floating_position.rs`.
- `ui.rs`: main GPUI presentation and user interaction. UI code must call the modules above rather than duplicating platform logic.
- `main.rs`: application composition/bootstrap only.

Keep platform handles out of persisted models. Raw HWND values may exist in runtime-only structs and must never be serialized.

## Secret-handling rules

- Never log passwords, decrypted vault JSON, credential blobs, DPAPI plaintext, or other secret material.
- `SavedConnection::Debug` must keep password redaction intact.
- Passwords must not be placed on the `mstsc.exe` command line, environment, temp `.rdp` files, or README/examples.
- For connection launch, store `TERMSRV/<host>` credentials through Windows Credential Manager (`CredWriteW`) and pass only host/port/options to `mstsc.exe`.
- Local vault and export files must contain encrypted bytes only. v0.1 uses current-user DPAPI, so exports are intentionally bound to the same Windows user profile.

## Windows integration constraints

- Identify MSTSC windows by resolving their owning process image and confirming `mstsc.exe`; never rely on title text alone.
- MSTSC discovery is **system-wide**, not launch-owned: enumerate the current visible top-level Windows desktop windows and include every window whose owning process is `mstsc.exe`, including sessions launched manually, from `.rdp` files, or by other applications.
- Never restrict the global MSTSC snapshot by `SavedConnection`, a PID list created by mstsc-mgr, child-process ownership, or any other "started by this app" bookkeeping.
- Window activation must restore a minimized window before bringing it to the foreground.
- Global shortcuts must use `RegisterHotKey`/`UnregisterHotKey` and must operate on the system-wide MSTSC snapshot described above.
- Numeric shortcuts map `Alt+Shift+1..9` to the current visible MSTSC window order.
- Cycling shortcuts are `Ctrl+Alt+Shift+Left/Right` and wrap around.
- Keepalive events currently use the same system-wide MSTSC snapshot, so when enabled they also target externally launched MSTSC sessions; they must not move the user's physical pointer or steal focus.
- Floating UI uses two independent top-level GPUI popups: a fixed native circular topmost ball and a separate topmost MSTSC-list popup. Never resize the ball window to display the list.
- Floating-ball dragging must be implemented through Win32 position updates and must not depend on a resizable combined popup or on MSTSC window ownership.

## UI/product constraints

- Main window must provide account creation/edit/delete/connect, settings, encrypted import, and encrypted export.
- Floating controller must be available after startup. Hovering the ball shows the independent current system-wide MSTSC window list; moving from the ball into the list must keep it stable and selectable.
- `always_show_tabs=true` keeps the independent MSTSC list visible next to the ball.
- Settings must expose floating controller, always-visible tabs, global hotkeys, close-to-tray, diagnostic file logging, keepalive enable, keepalive interval, and keepalive input type.
- Diagnostic logging defaults to enabled and writes `mstsc-mgr.log` next to the executable when that directory is writable; disabling it must stop subsequent log writes without logging secret data.
- User-visible errors should be surfaced as status/notifications; do not silently discard persistence/platform errors.

## Unsafe/code-quality rules

- Every `unsafe` block must have a nearby `// SAFETY:` comment describing the Win32 preconditions/invariants.
- Prefer small wrappers around unsafe APIs; keep unsafe out of UI/domain/config/logging code.
- Do not add a crate if the standard library or already-approved Windows/GPUI dependency is sufficient.
- No `unwrap()` / `expect()` in production code. Tests may use explicit assertion helpers rather than unwrap/expect where practical.
- Run before merging:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `cargo build --release`

## Mandatory version log

Every coding change must update `README.md` under **Development Log**. The heading format is exact:

`version x.y.z yyyy-MM-dd hh:mm:ss`

Rules:

1. Use Asia/Taipei local time.
2. A version entry describes the code in the same commit/PR.
3. Never rewrite an older entry to hide history; add a new version for later development.
4. Entries must be ordered in reverse chronological order (newest first). Insert each new version immediately below `## Development Log`; never append new entries at the bottom.
5. Patch version for fixes, minor version for backward-compatible features, major version for incompatible format/behavior changes.
6. Release tag must match Cargo package version (`vX.Y.Z` ↔ `X.Y.Z`).