use crate::domain::{AppSettings, KeepAliveInput, MstscWindow, SavedConnection};
use anyhow::{Context, Result, bail};
use std::{
    mem::size_of,
    path::Path,
    process::Command,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{CreateEllipticRgn, SetWindowRgn},
        Security::Credentials::{
            CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{
                AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, RegisterHotKey, SetFocus,
                UnregisterHotKey, VIRTUAL_KEY, VK_LBUTTON, VK_LEFT, VK_RIGHT,
            },
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, BringWindowToTop, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyMenu, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowW,
                GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HWND_MESSAGE,
                HWND_TOPMOST, IDI_APPLICATION, IsWindowVisible, LoadIconW, MF_SEPARATOR, MF_STRING,
                MSG, PostMessageW, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
                SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SW_RESTORE, SW_SHOWNOACTIVATE,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPos,
                ShowWindow, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
                WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_HOTKEY,
                WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NULL,
                WM_RBUTTONUP, WNDCLASSW,
            },
        },
    },
    core::{BOOL, HSTRING, PWSTR},
};

pub const MAIN_WINDOW_TITLE: &str = "mstsc-mgr";
pub const FLOATING_BALL_WINDOW_TITLE: &str = "mstsc-mgr-floating-ball";
pub const FLOATING_LIST_WINDOW_TITLE: &str = "mstsc-mgr-floating-list";

const HOTKEY_NUM_BASE: i32 = 0x5100;
const HOTKEY_PREVIOUS: i32 = 0x5200;
const HOTKEY_NEXT: i32 = 0x5201;
const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const TRAY_ICON_ID: u32 = 1;
const TRAY_MENU_OPEN: u16 = 1001;
const TRAY_MENU_EXIT: u16 = 1002;
const FLOATING_LIST_GAP: i32 = 10;
const DRAG_POLL_INTERVAL: Duration = Duration::from_millis(10);

static FORCE_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static FLOATING_DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);

pub type WindowSnapshot = Arc<RwLock<Vec<MstscWindow>>>;
pub type RuntimeSettings = Arc<RwLock<AppSettings>>;

pub fn launch_connection(connection: &SavedConnection) -> Result<()> {
    if connection.host.trim().is_empty() {
        bail!("host is required");
    }
    tracing::info!(host = %connection.host, port = connection.port, "launching MSTSC connection");
    if !connection.username.is_empty() && !connection.password.is_empty() {
        write_rdp_credential(connection)?;
    }

    let mut command = Command::new("mstsc.exe");
    command.arg(format!("/v:{}", connection.endpoint()));
    for arg in &connection.mstsc_args {
        if !arg.trim().is_empty() {
            command.arg(arg);
        }
    }
    command.spawn().context("failed to launch mstsc.exe")?;
    Ok(())
}

fn write_rdp_credential(connection: &SavedConnection) -> Result<()> {
    let target = wide_null(&connection.credential_target());
    let username = wide_null(&connection.username);
    let password: Vec<u16> = connection.password.encode_utf16().collect();
    let password_bytes = password
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|len| u32::try_from(len).ok())
        .context("password is too large")?;

    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr().cast_mut()),
        CredentialBlobSize: password_bytes,
        CredentialBlob: password.as_ptr().cast::<u8>().cast_mut(),
        Persist: CRED_PERSIST_ENTERPRISE,
        UserName: PWSTR(username.as_ptr().cast_mut()),
        ..Default::default()
    };

    // SAFETY: all pointers in CREDENTIALW refer to buffers that remain alive for the duration of
    // CredWriteW. Password length is supplied in bytes as required by the API.
    unsafe { CredWriteW(&credential, 0).context("CredWriteW failed")? };
    Ok(())
}

pub fn enumerate_mstsc_windows() -> Result<Vec<MstscWindow>> {
    let mut windows = Vec::<MstscWindow>::new();
    let state = LPARAM((&mut windows as *mut Vec<MstscWindow>) as isize);

    // SAFETY: LPARAM contains a valid mutable Vec pointer for the duration of synchronous
    // EnumWindows. The callback does not retain it.
    unsafe {
        EnumWindows(Some(enum_window_proc), state).context("EnumWindows failed")?;
    }
    Ok(windows)
}

unsafe extern "system" fn enum_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: caller passes a valid pointer to Vec<MstscWindow> in enumerate_mstsc_windows.
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<MstscWindow>) };
    // SAFETY: hwnd is supplied by EnumWindows and valid for the callback duration.
    if unsafe { !IsWindowVisible(hwnd).as_bool() } {
        return BOOL(1);
    }

    let mut pid = 0u32;
    // SAFETY: pid points to writable storage and hwnd comes from EnumWindows.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 || !process_is_mstsc(pid) {
        return BOOL(1);
    }

    let title = window_title(hwnd);
    windows.push(MstscWindow {
        hwnd: hwnd.0 as isize,
        pid,
        title: if title.trim().is_empty() {
            format!("mstsc.exe ({pid})")
        } else {
            title
        },
    });
    BOOL(1)
}

fn process_is_mstsc(pid: u32) -> bool {
    // SAFETY: pid came from GetWindowThreadProcessId; requested access only queries image name.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
    let Ok(process) = process else {
        return false;
    };

    let mut buffer = vec![0u16; 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: buffer is writable for `len` UTF-16 code units and process is a valid handle.
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: process was returned by OpenProcess and is no longer used after this point.
    unsafe {
        let _ = CloseHandle(process);
    }
    if result.is_err() {
        return false;
    }

    let image = String::from_utf16_lossy(&buffer[..len as usize]);
    Path::new(&image)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("mstsc.exe"))
}

fn window_title(hwnd: HWND) -> String {
    // SAFETY: hwnd is a live top-level window supplied by EnumWindows.
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; len as usize + 1];
    // SAFETY: buffer has len+1 UTF-16 units, sufficient for terminating NUL.
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied as usize])
}

pub fn activate_window(hwnd: isize) -> Result<()> {
    tracing::info!(hwnd, "activating MSTSC window");
    activate_hwnd(HWND(hwnd as *mut core::ffi::c_void))
}

fn activate_hwnd(hwnd: HWND) -> Result<()> {
    // SAFETY: hwnd is a top-level window handle discovered by EnumWindows/FindWindowW. Thread input
    // queues are attached only for the duration of this activation attempt and detached below.
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);

        let current_thread = GetCurrentThreadId();
        let foreground = GetForegroundWindow();
        let foreground_thread = if foreground.0.is_null() {
            0
        } else {
            GetWindowThreadProcessId(foreground, None)
        };
        let target_thread = GetWindowThreadProcessId(hwnd, None);

        let attached_foreground = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let attached_target = target_thread != 0
            && target_thread != current_thread
            && target_thread != foreground_thread
            && AttachThreadInput(current_thread, target_thread, true).as_bool();

        let result = (|| -> Result<()> {
            BringWindowToTop(hwnd).context("BringWindowToTop failed")?;
            if !SetForegroundWindow(hwnd).as_bool() {
                bail!("SetForegroundWindow was rejected by Windows foreground policy");
            }
            let _ = SetFocus(Some(hwnd));
            Ok(())
        })();

        if attached_target {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }
        if attached_foreground {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
        result
    }
}

fn find_window_by_title(title: &str) -> Result<HWND> {
    let title = HSTRING::from(title);
    // SAFETY: FindWindowW only reads the supplied UTF-16 title and returns a borrowed OS handle.
    unsafe { FindWindowW(None, &title).context("FindWindowW failed") }
}

pub fn hide_main_window() -> Result<()> {
    let hwnd = find_window_by_title(MAIN_WINDOW_TITLE)?;
    // SAFETY: hwnd is the application's current main top-level window.
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    Ok(())
}

pub fn show_main_window() -> Result<()> {
    let hwnd = find_window_by_title(MAIN_WINDOW_TITLE)?;
    activate_hwnd(hwnd)
}

pub fn take_force_exit_requested() -> bool {
    FORCE_EXIT_REQUESTED.swap(false, Ordering::SeqCst)
}

fn request_force_exit() -> Result<()> {
    FORCE_EXIT_REQUESTED.store(true, Ordering::SeqCst);
    let hwnd = find_window_by_title(MAIN_WINDOW_TITLE)?;
    // SAFETY: hwnd is the application's main window. WM_CLOSE asks GPUI to run the normal close
    // path; the atomic flag makes that path exit rather than hide to tray.
    unsafe {
        PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
            .context("PostMessageW WM_CLOSE failed")?;
    }
    Ok(())
}

pub fn configure_floating_ball_window() -> Result<()> {
    let hwnd = find_window_by_title(FLOATING_BALL_WINDOW_TITLE)?;
    let mut rect = RECT::default();
    // SAFETY: hwnd is the GPUI floating-ball top-level window. The region is sized from the actual
    // native window bounds and ownership transfers to Windows when SetWindowRgn succeeds.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .context("SetWindowPos floating ball topmost failed")?;
        GetWindowRect(hwnd, &mut rect).context("GetWindowRect floating ball failed")?;
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let region = CreateEllipticRgn(0, 0, width, height);
        if region.0.is_null() {
            bail!("CreateEllipticRgn failed for floating ball");
        }
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            bail!("SetWindowRgn failed for floating ball");
        }
    }
    tracing::info!("floating ball configured as native circular topmost window");
    Ok(())
}

pub fn configure_floating_list_window() -> Result<()> {
    let hwnd = find_window_by_title(FLOATING_LIST_WINDOW_TITLE)?;
    // SAFETY: hwnd is the independent GPUI MSTSC-list popup. Its current bounds are preserved while
    // it is promoted into the topmost Z band without activation.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .context("SetWindowPos floating list topmost failed")?;
    }
    Ok(())
}

fn point_in_rect(point: POINT, rect: RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

pub fn cursor_in_floating_controls() -> Result<bool> {
    let ball = find_window_by_title(FLOATING_BALL_WINDOW_TITLE)?;
    let mut cursor = POINT::default();
    let mut ball_rect = RECT::default();
    // SAFETY: cursor and rect are valid writable structures; ball is the application's live popup.
    unsafe {
        GetCursorPos(&mut cursor).context("GetCursorPos failed")?;
        GetWindowRect(ball, &mut ball_rect).context("GetWindowRect floating ball failed")?;
    }
    if point_in_rect(cursor, ball_rect) {
        return Ok(true);
    }

    let Ok(list) = find_window_by_title(FLOATING_LIST_WINDOW_TITLE) else {
        return Ok(false);
    };
    // SAFETY: list is a window owned by this application. Hidden list bounds are intentionally not
    // considered part of the hover region.
    if unsafe { !IsWindowVisible(list).as_bool() } {
        return Ok(false);
    }
    let mut list_rect = RECT::default();
    // SAFETY: list_rect is valid writable storage for the live list HWND.
    unsafe {
        GetWindowRect(list, &mut list_rect).context("GetWindowRect floating list failed")?;
    }
    Ok(point_in_rect(cursor, list_rect))
}

fn position_floating_list() -> Result<()> {
    let ball = find_window_by_title(FLOATING_BALL_WINDOW_TITLE)?;
    let list = find_window_by_title(FLOATING_LIST_WINDOW_TITLE)?;
    let mut ball_rect = RECT::default();
    let mut list_rect = RECT::default();
    // SAFETY: both HWNDs belong to this process and both RECT values are valid writable storage.
    unsafe {
        GetWindowRect(ball, &mut ball_rect).context("GetWindowRect floating ball failed")?;
        GetWindowRect(list, &mut list_rect).context("GetWindowRect floating list failed")?;
    }

    let list_width = (list_rect.right - list_rect.left).max(1);
    let list_height = (list_rect.bottom - list_rect.top).max(1);
    // SAFETY: GetSystemMetrics reads process-independent desktop metrics and has no pointer inputs.
    let (virtual_left, virtual_top, virtual_width, virtual_height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let virtual_right = virtual_left.saturating_add(virtual_width);
    let virtual_bottom = virtual_top.saturating_add(virtual_height);

    let right_x = ball_rect.right.saturating_add(FLOATING_LIST_GAP);
    let left_x = ball_rect
        .left
        .saturating_sub(FLOATING_LIST_GAP)
        .saturating_sub(list_width);
    let x = if right_x.saturating_add(list_width) <= virtual_right {
        right_x
    } else {
        left_x.max(virtual_left)
    };
    let max_y = virtual_bottom.saturating_sub(list_height).max(virtual_top);
    let y = ball_rect.top.clamp(virtual_top, max_y);

    // SAFETY: list is the independent list popup. Only its position/Z-order are changed; its fixed
    // GPUI layout size remains untouched, so moving the ball can never resize or shift the ball.
    unsafe {
        SetWindowPos(
            list,
            Some(HWND_TOPMOST),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .context("SetWindowPos floating list position failed")?;
    }
    Ok(())
}

pub fn set_floating_list_visible(visible: bool) -> Result<()> {
    let list = find_window_by_title(FLOATING_LIST_WINDOW_TITLE)?;
    if visible {
        position_floating_list()?;
        // SAFETY: list is the application's own popup. SW_SHOWNOACTIVATE preserves current focus,
        // and SetWindowPos keeps it in the topmost band without changing its size or position.
        unsafe {
            let _ = ShowWindow(list, SW_SHOWNOACTIVATE);
            SetWindowPos(
                list,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .context("SetWindowPos shown floating list failed")?;
        }
    } else {
        // SAFETY: list is the application's own popup and is simply hidden without destruction.
        unsafe {
            let _ = ShowWindow(list, SW_HIDE);
        }
    }
    tracing::info!(visible, "floating MSTSC list visibility changed");
    Ok(())
}

pub fn begin_floating_drag() -> Result<()> {
    let ball = find_window_by_title(FLOATING_BALL_WINDOW_TITLE)?;
    let mut start_rect = RECT::default();
    let mut start_cursor = POINT::default();
    // SAFETY: ball is the application's own popup and both output structures are valid writable
    // storage. These values seed the manual drag loop below.
    unsafe {
        GetWindowRect(ball, &mut start_rect).context("GetWindowRect before floating drag failed")?;
        GetCursorPos(&mut start_cursor).context("GetCursorPos before floating drag failed")?;
    }
    if FLOATING_DRAG_ACTIVE.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let ball_raw = ball.0 as isize;
    thread::spawn(move || {
        let ball = HWND(ball_raw as *mut core::ffi::c_void);
        let width = (start_rect.right - start_rect.left).max(1);
        let height = (start_rect.bottom - start_rect.top).max(1);
        tracing::info!("floating ball drag started");
        loop {
            // SAFETY: GetAsyncKeyState reads the global left-button state and requires no borrowed
            // pointers. The high bit is set while the button is physically held.
            let pressed = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
            if !pressed {
                break;
            }

            let mut cursor = POINT::default();
            // SAFETY: cursor is valid writable storage for the current pointer position.
            if unsafe { GetCursorPos(&mut cursor) }.is_err() {
                break;
            }

            // SAFETY: GetSystemMetrics only reads virtual-desktop dimensions.
            let (virtual_left, virtual_top, virtual_width, virtual_height) = unsafe {
                (
                    GetSystemMetrics(SM_XVIRTUALSCREEN),
                    GetSystemMetrics(SM_YVIRTUALSCREEN),
                    GetSystemMetrics(SM_CXVIRTUALSCREEN),
                    GetSystemMetrics(SM_CYVIRTUALSCREEN),
                )
            };
            let max_x = virtual_left
                .saturating_add(virtual_width)
                .saturating_sub(width)
                .max(virtual_left);
            let max_y = virtual_top
                .saturating_add(virtual_height)
                .saturating_sub(height)
                .max(virtual_top);
            let x = start_rect
                .left
                .saturating_add(cursor.x.saturating_sub(start_cursor.x))
                .clamp(virtual_left, max_x);
            let y = start_rect
                .top
                .saturating_add(cursor.y.saturating_sub(start_cursor.y))
                .clamp(virtual_top, max_y);

            // SAFETY: ball remains owned by this process for the application lifetime. The manual
            // drag changes only position and preserves the native circular region and topmost state.
            unsafe {
                let _ = SetWindowPos(
                    ball,
                    Some(HWND_TOPMOST),
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            let _ = position_floating_list();
            thread::sleep(DRAG_POLL_INTERVAL);
        }
        FLOATING_DRAG_ACTIVE.store(false, Ordering::SeqCst);
        tracing::info!("floating ball drag finished");
    });
    Ok(())
}

pub fn start_tray_worker() {
    thread::spawn(|| {
        if let Err(error) = tray_message_loop() {
            tracing::error!(%error, "system tray worker stopped");
        }
    });
}

fn tray_message_loop() -> Result<()> {
    // SAFETY: this worker owns the registered window class, message-only HWND, tray icon and message
    // loop for their full lifetime. All pointers reference static/stack data kept alive per call.
    unsafe {
        let module = GetModuleHandleW(None).context("GetModuleHandleW failed")?;
        let instance: HINSTANCE = module.into();
        let class_name = windows::core::w!("MstscMgrTrayWindow");
        let class = WNDCLASSW {
            lpfnWndProc: Some(tray_window_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            bail!("RegisterClassW failed for tray window");
        }

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            windows::core::w!("mstsc-mgr tray"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
        .context("CreateWindowExW failed for tray window")?;

        let icon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MESSAGE,
            hIcon: icon,
            ..Default::default()
        };
        let tip = wide_null("mstsc-mgr");
        let tip_len = tip.len().min(data.szTip.len());
        data.szTip[..tip_len].copy_from_slice(&tip[..tip_len]);
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            let _ = DestroyWindow(hwnd);
            bail!("Shell_NotifyIconW(NIM_ADD) failed");
        }
        tracing::info!("system tray icon created");

        let mut message = MSG::default();
        loop {
            let code = GetMessageW(&mut message, None, 0, 0);
            if code.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}

fn show_tray_context_menu(hwnd: HWND) -> Result<()> {
    // SAFETY: menu handles are created and destroyed in this function. The popup is owned by the
    // tray message window and Windows sends its WM_COMMAND selection back to that HWND.
    unsafe {
        let menu = CreatePopupMenu().context("CreatePopupMenu failed")?;
        let result = (|| -> Result<()> {
            AppendMenuW(
                menu,
                MF_STRING,
                usize::from(TRAY_MENU_OPEN),
                windows::core::w!("Open mstsc-mgr"),
            )
            .context("AppendMenuW open failed")?;
            AppendMenuW(menu, MF_SEPARATOR, 0, None).context("AppendMenuW separator failed")?;
            AppendMenuW(
                menu,
                MF_STRING,
                usize::from(TRAY_MENU_EXIT),
                windows::core::w!("Exit"),
            )
            .context("AppendMenuW exit failed")?;

            let mut point = POINT::default();
            GetCursorPos(&mut point).context("GetCursorPos failed")?;
            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                Some(0),
                hwnd,
                None,
            );
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            Ok(())
        })();
        let _ = DestroyMenu(menu);
        result
    }
}

unsafe extern "system" fn tray_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == TRAY_CALLBACK_MESSAGE {
        match lparam.0 as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                let _ = show_main_window();
                return LRESULT(0);
            }
            WM_RBUTTONUP => {
                let _ = show_tray_context_menu(hwnd);
                return LRESULT(0);
            }
            _ => {}
        }
    } else if message == WM_COMMAND {
        match (wparam.0 & 0xffff) as u16 {
            TRAY_MENU_OPEN => {
                let _ = show_main_window();
                return LRESULT(0);
            }
            TRAY_MENU_EXIT => {
                tracing::info!("exit requested from tray menu");
                let _ = request_force_exit();
                return LRESULT(0);
            }
            _ => {}
        }
    }
    // SAFETY: unhandled messages are forwarded to the default window procedure for this valid HWND.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub fn start_window_watcher(snapshot: WindowSnapshot) {
    thread::spawn(move || {
        let mut last_count = usize::MAX;
        loop {
            if let Ok(current) = enumerate_mstsc_windows() {
                if current.len() != last_count {
                    last_count = current.len();
                    tracing::info!(count = last_count, "system-wide MSTSC window snapshot changed");
                }
                if let Ok(mut guard) = snapshot.write() {
                    *guard = current;
                }
            }
            thread::sleep(Duration::from_millis(700));
        }
    });
}

pub fn start_keepalive_worker(snapshot: WindowSnapshot, settings: RuntimeSettings) {
    thread::spawn(move || {
        let mut last_sent = Instant::now();
        loop {
            let current = settings
                .read()
                .map(|guard| guard.clone())
                .unwrap_or_default();
            let interval = Duration::from_secs(current.keepalive_interval_seconds.max(5));
            if current.keepalive_enabled && last_sent.elapsed() >= interval {
                if let Ok(windows) = snapshot.read() {
                    for item in windows.iter() {
                        post_keepalive(item.hwnd, current.keepalive_input);
                    }
                    tracing::info!(count = windows.len(), "keepalive messages sent");
                }
                last_sent = Instant::now();
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

fn post_keepalive(hwnd: isize, input: KeepAliveInput) {
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: HWNDs come from the current MSTSC snapshot. PostMessageW is asynchronous and does
    // not retain references to Rust memory. Messages target the RDP client window only.
    unsafe {
        match input {
            KeepAliveInput::MouseMove => {
                let _ = PostMessageW(Some(hwnd), WM_MOUSEMOVE, WPARAM(0), LPARAM(0));
            }
            KeepAliveInput::ShiftKey => {
                let _ = PostMessageW(Some(hwnd), WM_KEYDOWN, WPARAM(0x10), LPARAM(0));
                let _ = PostMessageW(Some(hwnd), WM_KEYUP, WPARAM(0x10), LPARAM(0));
            }
        }
    }
}

pub fn start_hotkey_worker(snapshot: WindowSnapshot, settings: RuntimeSettings) {
    thread::spawn(move || {
        let registrations = register_hotkeys();
        tracing::info!(count = registrations.len(), "global hotkeys registered");
        let mut message = MSG::default();
        loop {
            // SAFETY: message points to initialized writable storage. This thread owns the message
            // loop and registered hotkeys.
            let code = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if code.0 <= 0 {
                break;
            }
            if message.message != WM_HOTKEY {
                continue;
            }
            let enabled = settings
                .read()
                .map(|guard| guard.global_hotkeys)
                .unwrap_or(false);
            if !enabled {
                continue;
            }
            let id = message.wParam.0 as i32;
            if (HOTKEY_NUM_BASE + 1..=HOTKEY_NUM_BASE + 9).contains(&id) {
                let index = (id - HOTKEY_NUM_BASE - 1) as usize;
                activate_index(&snapshot, index);
            } else if id == HOTKEY_PREVIOUS {
                activate_relative(&snapshot, -1);
            } else if id == HOTKEY_NEXT {
                activate_relative(&snapshot, 1);
            }
        }

        for id in registrations {
            // SAFETY: unregistering IDs registered by this worker on this same thread.
            unsafe {
                let _ = UnregisterHotKey(None, id);
            }
        }
    });
}

fn register_hotkeys() -> Vec<i32> {
    let mut registered = Vec::new();
    for number in 1..=9i32 {
        let id = HOTKEY_NUM_BASE + number;
        let vk = VIRTUAL_KEY((b'0' + number as u8) as u16);
        // SAFETY: thread-level hotkey registration uses no HWND; IDs are unique in this thread.
        if unsafe { RegisterHotKey(None, id, MOD_ALT | MOD_SHIFT, u32::from(vk.0)) }.is_ok() {
            registered.push(id);
        }
    }
    // SAFETY: same thread-level registration invariants as numeric hotkeys.
    if unsafe {
        RegisterHotKey(
            None,
            HOTKEY_PREVIOUS,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT,
            u32::from(VK_LEFT.0),
        )
    }
    .is_ok()
    {
        registered.push(HOTKEY_PREVIOUS);
    }
    // SAFETY: same thread-level registration invariants as numeric hotkeys.
    if unsafe {
        RegisterHotKey(
            None,
            HOTKEY_NEXT,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT,
            u32::from(VK_RIGHT.0),
        )
    }
    .is_ok()
    {
        registered.push(HOTKEY_NEXT);
    }
    registered
}

fn activate_index(snapshot: &WindowSnapshot, index: usize) {
    if let Ok(windows) = snapshot.read()
        && let Some(item) = windows.get(index)
    {
        let _ = activate_window(item.hwnd);
    }
}

fn activate_relative(snapshot: &WindowSnapshot, delta: isize) {
    let Ok(windows) = snapshot.read() else {
        return;
    };
    if windows.is_empty() {
        return;
    }
    // SAFETY: no parameters; simply queries Windows foreground HWND.
    let current = unsafe { GetForegroundWindow() };
    let current_raw = current.0 as isize;
    let current_index = windows
        .iter()
        .position(|item| item.hwnd == current_raw)
        .unwrap_or(0);
    let len = windows.len() as isize;
    let next = (current_index as isize + delta).rem_euclid(len) as usize;
    if let Some(item) = windows.get(next) {
        let _ = activate_window(item.hwnd);
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
