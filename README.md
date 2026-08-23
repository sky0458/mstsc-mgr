# mstsc-mgr

A Windows 10+ native MSTSC manager written in **Rust + GPUI**, modeled after the useful parts of RDM's **External** mode: the RDP client remains Microsoft's `mstsc.exe`, while mstsc-mgr owns saved connections, secure credentials, global switching and a floating session controller.

## Features

- Native Rust/GPUI desktop app; no .NET/WPF/WinUI, Electron, Java, Python or Node runtime. Windows release builds use the GUI subsystem, so launching `mstsc-mgr.exe` does not open a console/terminal window.
- Uses an original project-specific application icon with no third-party product/logo artwork. A multi-resolution Windows ICO is embedded directly into `mstsc-mgr.exe` at build time and is also shipped in the release package.
- Save multiple RDP connections with host, port, username, password and optional MSTSC arguments.
- Passwords and the local vault are encrypted with Windows DPAPI; plaintext secrets are not written to disk.
- Launches external `mstsc.exe` and writes `TERMSRV/<host>` credentials using Windows Credential Manager instead of putting passwords on the command line.
- Settings dialog for floating controller, floating opacity, persistent tabs, global hotkeys, close-to-tray, diagnostic logging and keepalive behavior.
- Diagnostic logging defaults to enabled and writes `mstsc-mgr.log` next to `mstsc-mgr.exe` when that directory is writable. The Settings switch can stop subsequent log writes immediately; passwords, decrypted vault data and credential blobs are never logged.
- Encrypted vault import/export. **v0.1/v0.2 exports are DPAPI current-user bound**, so importing requires the same Windows user profile.
- The floating controller is two independent GPUI/Win32 top-level components: a fixed 64px native circular topmost RDP ball and a separate compact MSTSC-session popup. Showing the list never resizes or moves the ball.
- The RDP ball receives a native elliptical Windows region, so its actual HWND hit/paint region is circular rather than a transparent rectangle with a rounded child drawn inside it.
- Floating-controller opacity is configurable from 10% to 100% with a Settings slider and defaults to 50%; the same value is applied to both the RDP ball and its hover session popup.
- The floating controller remains enabled by default and can be hidden or shown from Settings at runtime without restarting the application.
- Right-clicking the floating ball opens an independent compact custom GPUI menu window with `Show main window`, `Close floating controller`, and `Exit`. The menu is sized like a context menu, stays tight against the ball, and dismisses when either mouse button is pressed elsewhere.
- Hover visibility is driven by cursor polling across the ball and the independent list with a leave grace period. The list stays stable while the pointer moves from the ball into a session row, including when the MSTSC list is empty.
- The session popup is forced to a compact 240px width, resizes vertically to the number of visible MSTSC sessions, and is anchored directly below the floating ball (falling back above only when the bottom screen edge has insufficient room).
- The floating ball uses the v0.2.10 visible 64×64 creation/configuration lifecycle so the native ellipse is always derived from settled ball bounds. Startup position is re-applied later using `SWP_NOSIZE` only, and native drag movement is watched so the final X/Y is persisted without depending on GPUI mouse-up delivery.
- MSTSC discovery is system-wide: sessions are included whether they were launched by mstsc-mgr, opened manually through `mstsc.exe`, opened from an `.rdp` file, or started by another application. Saved connections are **not** used as a filter for window discovery.
- Current MSTSC windows are placed into a stable PID/HWND order before the shared snapshot is published. Hover-list numbers and `Alt+Shift+1..9` therefore use the same ordering and no longer change merely because focus or Windows Z-order changes.
- Click a floating list row to restore/activate its MSTSC window. Activation temporarily joins the relevant Windows input queues to satisfy foreground-window restrictions, then detaches immediately.
- Global switching operates on that complete system-wide set of visible MSTSC windows:
  - `Alt+Shift+1..9`: activate the Nth current visible MSTSC window.
  - `Ctrl+Alt+Shift+Left/Right`: cycle through current MSTSC windows with wrap-around.
- Optional keepalive with a targeted `WM_MOUSEMOVE` or Shift key message sent only to enumerated MSTSC HWNDs. Because discovery is system-wide, keepalive currently also applies to externally launched MSTSC sessions. It does not move the physical mouse or steal foreground focus.
- The main window uses the native Windows title bar and is movable/resizable. Closing it defaults to hiding it in the system tray; this behavior can be disabled in Settings. Left-clicking the tray icon restores the main window, and right-clicking opens a native menu with Open and Exit actions.
- GitHub Actions CI on Windows and tagged release packaging.

## Security model

Local data is stored under `%LOCALAPPDATA%\mstsc-mgr`:

- `settings.json`: non-secret settings.
- `vault.dpapi`: DPAPI-encrypted serialized connection vault.

Portable diagnostic output is written beside the executable when enabled:

- `mstsc-mgr.log`: runtime diagnostics only; no passwords, decrypted vault JSON or credential blobs.

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

A SemVer tag must match `Cargo.toml` (`vX.Y.Z` ↔ `X.Y.Z`). `.github/workflows/release.yml` builds on `windows-2025` and publishes two x64 ZIP assets from the same release executable:

- `mstsc-mgr-windows-x64.zip`: standard package for current Windows 10/11 and newer Windows Server versions. It relies on the system ICU shipped by modern Windows.
- `mstsc-mgr-windows-legacy-x64.zip`: compatibility package for Windows Server 2016 / Windows 10 1607. It additionally ships a tiny project-built `icuuc.dll` compatibility shim that implements the only ICU entry point imported by GPUI 0.2.2 (`u_strlen`).

CI and Release validate that `mstsc-mgr.exe` imports `icuuc.dll!u_strlen` and that the compatibility shim exports `u_strlen` before either package is uploaded. The standard package deliberately does not contain the shim.

Two release paths are supported:

- Push a matching SemVer tag such as `v0.2.12`.
- Merge to `main` with a merge commit whose message starts with `release:`. The workflow resolves the Cargo version, creates/publishes the matching tag and GitHub Release from that exact `main` commit.

## Development Log

Entries are ordered newest to oldest.

### version 0.2.12 2026-08-23 11:59:00

- Rolled the floating-ball startup lifecycle back to the v0.2.10 behavior: the 64×64 popup is created with its normal visibility and native circular configuration instead of being created hidden, eliminating the oversized temporary HWND/elliptical region regression introduced in v0.2.11.
- Reimplemented startup coordinate recovery as a delayed position-only stabilization pass. It validates that the native ball bounds are already small/settled, then uses `SetWindowPos(..., SWP_NOSIZE)` so saved/default placement can never resize or reshape the floating ball.
- Reimplemented coordinate persistence with a native position watcher that detects actual ball movement while the left button is held and saves the final X/Y on release, while leaving the v0.2.10 drag, click, hover, menu, opacity and circular-region code unchanged.
- Added dual Windows release packages: the standard package remains unchanged for current Windows versions, while a separate Windows Server 2016 / Windows 10 1607 compatibility ZIP includes a minimal `icuuc.dll` shim providing GPUI 0.2.2's sole ICU import, `u_strlen`.
- Added CI/Release PE contract checks for the ICU import/export and publish both `mstsc-mgr-windows-x64.zip` and `mstsc-mgr-windows-legacy-x64.zip` from the same tested executable.

### version 0.2.11 2026-08-23 11:32:00

- Changed floating-ball startup to create the popup hidden, wait until the GPUI/Win32 HWND initialization has settled, then apply the saved/default native coordinates before making the controller visible; this prevents later GPUI bounds initialization from overwriting the position back to the top-left corner.
- Moved coordinate persistence onto a native drag watcher that observes actual HWND movement while the left button is held and writes the final X/Y only after release, removing the unreliable dependency on GPUI receiving `on_mouse_up` after a manual Win32 drag.
- Changed the first-run fallback to the primary display's right edge at roughly 30% of screen height, matching the intended right-side placement while preserving virtual-desktop bounds validation for saved coordinates.

### version 0.2.10 2026-08-23 11:16:00

- Forced the simulated floating-ball context menu to a compact 180px context-menu footprint with content-derived height, native Win32 size enforcement, and a 2px attachment gap that flips sides at screen edges.
- Added automatic menu dismissal when either the left or right mouse button is pressed outside both the floating menu and the floating ball.
- Added persisted floating-ball X/Y coordinates after dragging, startup restoration with virtual-desktop bounds validation, and a first-run fallback near the primary display's right edge with vertical centering.

### version 0.2.9 2026-08-23 08:58:00

- Added a new original project-specific application icon designed specifically for mstsc-mgr's remote-session/security use case, without incorporating third-party product logos or trademark artwork.
- Added a multi-resolution Windows ICO asset and a Windows resource build step using `winresource`, so the icon is embedded directly into the release `mstsc-mgr.exe` and appears as the executable/application icon in Windows.
- Updated release packaging to ship `mstsc-mgr.ico` alongside the executable, README and LICENSE for reuse by shortcuts or future installers.

### version 0.2.8 2026-08-23 02:42:00

- Made the existing `Show floating controller` setting fully runtime-aware while keeping its default enabled: the ball, hover list and custom menu windows are created once, and saving the setting hides or restores the controller without requiring an application restart.
- Added a right-click menu as a completely separate GPUI popup window rather than a native `TrackPopupMenu`. It provides `Show main window`, `Close floating controller`, and `Exit`; closing the controller also persists the floating setting as disabled so it can be explicitly re-enabled from Settings.
- Preserved the v0.2.7 floating-ball geometry unchanged: the original 64×64 GPUI window options, native elliptical region configuration, topmost behavior and manual drag sizing/position logic are not resized or replaced by the menu implementation.

### version 0.2.7 2026-08-23 00:48:00

- Reset the implementation tree to the exact `v0.2.3` baseline before applying this version, intentionally removing the later v0.2.4-v0.2.6 floating-controller implementation changes while retaining their historical log entries below.
- Added only one floating-controller UI enhancement on top of v0.2.3: a persisted 10-100% opacity slider in Settings, defaulting to 50%, applied consistently to the RDP ball and the hover MSTSC-session popup.
- Stabilized the shared system-wide MSTSC snapshot by sorting sessions by PID and HWND before publishing it. The hover-list numbers and `Alt+Shift+1..9` now consume the same deterministic order, so focus/Z-order changes no longer reshuffle the numeric mapping.

### version 0.2.6 2026-08-23 00:16:00

- Made the floating ball DPI-aware: the GPUI surface remains 64×64 logical pixels while its native Win32 HWND is forced to the matching physical square size using the current GPUI window scale factor before the elliptical window region is applied. This keeps the visible ball and native hit region truly circular at Windows display scaling values such as 125%, 150%, and 200%.
- Restored the floating-ball context menu to the floating window's own UI thread and trigger it on right-button release instead of right-button press. `TPM_NONOTIFY | TPM_RETURNCMD` is retained so menu commands do not re-enter the GPUI popup as `WM_COMMAND` messages.
- Kept left-click and right-click paths completely separate and post `WM_NULL` after menu tracking, so the menu can be selected and dismissed normally without invoking the normal left-click show-main-window path or exiting the application accidentally.

### version 0.2.5 2026-08-22 23:50:03

- Restored the floating RDP ball to a strict 64×64 logical and native size before it is shown, preventing a hidden GPUI startup window size from being converted into the giant elliptical region seen in v0.2.4.
- Moved the floating-ball native `TrackPopupMenu` call off the GPUI mouse-event thread and added `TPM_NONOTIFY | TPM_RETURNCMD`, avoiding native-menu command re-entry into the GPUI popup lifecycle.
- Separated mouse actions explicitly: right-click propagation is stopped and only a left-button release can execute the normal show-main-window action, so selecting a floating-ball context-menu item no longer shares the click path or unexpectedly exits the application.

### version 0.2.4 2026-08-22 22:50:30

- Added a floating-controller opacity slider in Settings with a 10-100% range and a 50% default. The selected opacity is persisted and applied consistently to both the circular RDP ball and the hover session popup.
- Made the existing floating-controller setting runtime-aware: the controller remains enabled by default, can be hidden from Settings, and can be shown again without restarting because the two floating windows are created once and then controlled through native Show/Hide operations.
- Added a native right-click menu to the floating ball with `Show main window`, `Close floating controller`, and `Exit`. Closing hides only the floating controller, while Exit terminates the entire application even when close-to-tray is enabled.
- Stabilized hover-list and numeric-hotkey numbering by sorting every system-wide MSTSC snapshot by PID and HWND before publishing it to the shared list used by both the popup and `Alt+Shift+1..9`. Changing focus/Z-order no longer reassigns session numbers.

### version 0.2.3 2026-08-22 22:04:00

- Forced the independent MSTSC popup to a compact 240px width and changed its height to follow the current visible session count instead of retaining a large fixed 360×420 surface. The hidden popup is resized through GPUI before it is shown, preventing the near-full-screen native popup seen on affected Windows environments.
- Repositioned the popup directly below the floating RDP ball with right-edge alignment and virtual-desktop clamping; it only flips above the ball when there is not enough space below.
- Added normal floating-ball click behavior to restore/activate the main mstsc-mgr window.
- Added a drag-distance threshold so a real floating-ball drag suppresses the subsequent click action, while an ordinary click opens the main window.
- Kept hover detection spanning both the ball and the independent compact list, preserving stable row selection while moving the pointer into the popup.

### version 0.2.2 2026-08-22 20:48:33

- Added `mstsc-mgr.log` in the executable directory as the default diagnostic log target and added a Settings switch that can stop subsequent log writes immediately. Logging records startup, MSTSC snapshot count changes, activation attempts, keepalive activity, hotkey registration, floating-list visibility/dragging and tray lifecycle without logging passwords or decrypted vault contents.
- Replaced the combined resizable floating popup with two independent GPUI/Win32 top-level components: a fixed floating ball HWND and a separate fixed-size MSTSC-list HWND. The list can appear/disappear without changing the ball window bounds, eliminating the left/right jumping caused by resize anchoring.
- Applied a native Win32 elliptical window region to the floating-ball HWND so the actual top-level window is circular, not merely a rounded element rendered inside a rectangular transparent popup.
- Reworked hover behavior to poll the cursor across both independent windows with a 500ms leave grace. Moving from the ball into the list no longer collapses the list, and empty/non-empty session states use the same stable popup.
- Replaced caption-message dragging with an explicit Win32 drag loop using global left-button state, cursor deltas and `SetWindowPos`, keeping the ball inside the virtual desktop and moving the separate list alongside it when visible.
- Removed the obsolete combined `FloatingController` implementation from `ui.rs` and made the split floating architecture an explicit contributor constraint.

### version 0.2.1 2026-08-22 19:59:30

- Increased the floating popup safety margin and ball diameter so the complete RDP control is rendered inside the Windows client area as a true circle instead of being clipped into a rounded rectangle.
- Replaced resize-on-hover feedback with cursor-position polling plus a short leave grace period. Expanding the native popup no longer immediately generates the opposite hover state, eliminating the repeated jump/flicker that made MSTSC rows impossible to select.
- Kept the expanded window anchored to the ball and continued using the same system-wide MSTSC snapshot, so both the empty-state row and detected-session rows remain stable while the pointer moves from the ball into the list.
- Added a native tray context menu on right-click with `Open mstsc-mgr` and `Exit`. Exit now bypasses the normal close-to-tray behavior and asks the GPUI main window to terminate cleanly.

### version 0.2.0 2026-08-22 18:48:00

- Reworked the floating controller into a compact native popup: collapsed bounds now match the ball instead of reserving a 360×560 invisible/white rectangle, and the GPUI `Root` wrapper is no longer used for the transparent popup.
- Added Win32 caption-drag behavior to the floating ball so it can be moved freely on Windows, and promoted/resized it with `HWND_TOPMOST` so it stays above normal application windows.
- Hardened MSTSC activation for both floating tabs and global hotkeys by temporarily attaching the app thread input queue to the current foreground/target threads before restoring and foregrounding the selected RDP window.
- Switched Windows builds to the GUI subsystem so launching the release executable no longer creates an accompanying cmd/terminal console window.
- Restored normal main-window movement/resizing by using the native Windows title bar instead of the undraggable transparent component title bar configuration.
- Added a native system-tray icon and a `Close main window to system tray` setting. The setting defaults to enabled; clicking X hides the main window, clicking the tray icon restores the main window, and disabling the setting makes X exit the app.
- Added the Win32 Shell and LibraryLoader bindings required for tray integration while keeping the project dependency/runtime boundary unchanged.

### version 0.1.5 2026-08-22 17:31:11

- Fixed the final Rust 1.98 Clippy diagnostics by deriving `Default` for `KeepAliveInput` and removing unnecessary mutable WinAPI input references.
- Restored the strict `cargo fmt --all -- --check` CI gate before merge/release.
- Made reverse-chronological README logging an explicit contributor rule: every new version is inserted at the top of Development Log.
- Extended the release workflow so an explicit `release:` merge commit on `main` can publish the exact merged Cargo version while preserving normal SemVer-tag releases.

### version 0.1.4 2026-08-22 17:24:05

- Fixed the remaining GPUI compile error by importing `gpui_component::scroll::ScrollableElement`, which provides `overflow_y_scrollbar()` for `gpui::Div`.
- No MSTSC discovery or switching behavior changed; the system-wide external-session guarantee introduced in 0.1.3 remains intact.

### version 0.1.3 2026-08-22 17:21:04

- Verified that MSTSC discovery is already system-wide: `EnumWindows` scans the desktop, each owning PID is resolved to its process image, and any visible top-level window owned by `mstsc.exe` is included regardless of who launched it.
- Confirmed that global numeric/cycle hotkeys consume this same system-wide snapshot, so manually launched, `.rdp`-launched, and third-party-launched MSTSC sessions participate in switching.
- Added an explicit architecture constraint forbidding future implementations from filtering the global snapshot by saved connections or mstsc-mgr-owned PIDs.
- Documented that keepalive currently consumes the same global MSTSC snapshot and therefore also applies to externally launched MSTSC sessions when enabled.

### version 0.1.2 2026-08-22 17:01:15

- Fixed the first real Windows compile diagnostics: moved global hotkey APIs to `Win32::UI::Input::KeyboardAndMouse`, used `windows::core::BOOL` for the `EnumWindows` callback, and passed optional HWNDs to `PostMessageW`.
- Corrected DPAPI `LocalFree` calls to use the typed `HLOCAL` wrapper required by `windows` 0.61.
- Updated the GPUI scroll container to the 0.2.2 `overflow_y_scrollbar()` API.
- Kept the temporary non-blocking format diagnostic while Windows CI validates the corrected Win32/GPUI API surface.

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
