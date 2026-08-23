use crate::platform;
use anyhow::{Context as _, Result};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::{EnableWindow, ReleaseCapture},
        WindowsAndMessaging::{
            BringWindowToTop, FindWindowW, GWL_STYLE, GetShellWindow, GetWindowLongW, SW_MINIMIZE,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, WS_CAPTION, WS_MAXIMIZEBOX,
            WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
        },
    },
};
use windows::core::HSTRING;

fn find_main_window() -> Result<HWND> {
    let title = HSTRING::from(platform::MAIN_WINDOW_TITLE);
    // SAFETY: FindWindowW only reads the supplied title and returns an OS-owned HWND.
    unsafe { FindWindowW(None, &title).context("FindWindowW main window failed") }
}

/// Refreshes the main window's standard native frame and releases any stale mouse capture.
/// This is intentionally separate from MSTSC activation because the app's own GPUI HWND does not
/// need the aggressive cross-thread input attachment used for external RDP windows.
pub fn repair_main_window_frame() -> Result<()> {
    let hwnd = find_main_window()?;
    let required_style =
        (WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0 as i32;

    // SAFETY: hwnd is the application's main top-level window. The operations only re-enable the
    // window, restore the normal frame bits and ask Windows to recalculate the non-client frame.
    unsafe {
        let _ = ReleaseCapture();
        let _ = EnableWindow(hwnd, true);
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        if style & required_style != required_style {
            let _ = SetWindowLongW(hwnd, GWL_STYLE, style | required_style);
        }
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
        .context("SetWindowPos main frame refresh failed")?;
    }
    Ok(())
}

/// Minimizes every currently visible MSTSC window and returns to the host desktop.
/// mstsc-mgr itself is hidden to the tray while the floating controller stays available.
pub fn host_desktop() -> Result<usize> {
    let windows = platform::enumerate_mstsc_windows()?;
    for window in &windows {
        let hwnd = HWND(window.hwnd as *mut core::ffi::c_void);
        // SAFETY: HWNDs come from the current system-wide MSTSC enumeration. ShowWindow only
        // changes their display state and does not retain any Rust references.
        unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }
    }

    let _ = platform::hide_main_window();

    // SAFETY: GetShellWindow returns the shell-owned desktop HWND. Foreground promotion is
    // best-effort because Windows may reject it under normal foreground-lock policy.
    unsafe {
        let shell = GetShellWindow();
        if !shell.0.is_null() {
            let _ = BringWindowToTop(shell);
            let _ = SetForegroundWindow(shell);
        }
    }

    tracing::info!(
        count = windows.len(),
        "switched to host desktop by minimizing MSTSC windows"
    );
    Ok(windows.len())
}
