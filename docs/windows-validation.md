# Windows regression checks

Run these checks on Windows for the GPUI version and on Server 2016 for the independent native version. Linux model tests and Windows cross-compilation do not exercise desktop behavior.

## Automated checks

From each version's checkout:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

Keep the Win2016 branch's existing dependency-surface CI guard: its release executable must not import D3D11, DXGI, DirectComposition, ICU or WebView2.

## Shared accounts (both versions)

1. Back up the existing data. Start with an existing data file and confirm its connections and individual passwords still load.
2. Add a shared account. Link two connections with different hosts to it and save. Restart the application; both should retain the same account selection.
3. Change that account's username/password once. Connect to both hosts and confirm the new credentials are used. Existing RDP sessions should stay connected.
4. Attempt to delete the shared account while referenced: deletion must fail and leave both the disk and displayed data intact. Unlink both connections, then delete it successfully.
5. GPUI: export and import an encrypted vault containing shared accounts in the same Windows user profile. Verify references and passwords survive. The native branch has separate storage and does not import GPUI vault files.
6. Make the data directory unwritable and try to save an edit. An error must appear; restarting must retain the last successfully saved data. A corrupt or undecryptable store must produce a startup error instead of opening an empty writable store.

## Floating controller (GPUI)

1. Move the ball, disable/re-enable the controller, then restart. Its position, 64px geometry and circular shape should remain unchanged.
2. Activate several windowed and fullscreen MSTSC sessions repeatedly, including sessions launched outside the tool. The ball should recover to the topmost Z-order within roughly one second without stealing keyboard focus.
3. Hover the ball and move into its list, then open its context menu. The list/menu must remain above the ball and be selectable. Closing them must not make the periodic topmost recovery show them again.
4. Disable the controller and switch foreground sessions. It must remain hidden. Repeat with two copies of the tool running: each should manipulate only its own floating windows.
5. Repeat after changing display resolution and with multiple monitors. Secure desktops and exclusive fullscreen applications may prevent ordinary desktop overlays.

## Dragging

1. GPUI: drag the main native title bar and the application heading after restoring from the ball and from the tray. Open Connection, Shared account and Settings dialogs; drag their headings in all directions. The heading must remain reachable, and input fields/buttons must still work after release, including release outside the client area.
2. Native: open an editor and drag its native title bar repeatedly. The owner must be disabled until the editor closes. Test Save, Cancel, title-bar Close, Tab, Enter and Escape, and confirm another editor cannot be opened concurrently.

## Session labels (both versions)

1. Launch a connection with a Unicode saved name and a non-default port. The main MSTSC title should become `saved name (host:port)` without renaming credential dialogs.
2. Toggle fullscreen, reconnect and switch focus; the label should be reapplied if MSTSC rewrites it. Check the GPUI floating list and Alt+Tab independently of the fullscreen connection bar, which MSTSC may label separately.
3. Launch another connection to the same host with a different saved name, and launch an unrelated session manually. Labels must stay attached to their original main container and not alter the manual session.
4. Close a labelled session; later processes/windows must not inherit its label. Elevated clients and handoff to an existing process may prevent renaming and should not prevent connection launch or global discovery.
