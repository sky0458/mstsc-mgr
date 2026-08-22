use crate::domain::{AppSettings, KeepAliveInput, MstscWindow, SavedConnection};
use anyhow::{Context, Result, bail};
use std::{
    mem::size_of,
    path::Path,
    process::Command,
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
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
                MOD_ALT, MOD_CONTROL, MOD_SHIFT, RegisterHotKey, ReleaseCapture, SetFocus,
                UnregisterHotKey, VIRTUAL_KEY, VK_LEFT, VK_RIGHT,
            },
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                EnumWindows, FindWindowW, GetForegroundWindow, GetMessageW, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HTCAPTION,
                HWND_MESSAGE, HWND_TOPMOST, IDI_APPLICATION, IsWindowVisible, LoadIconW, MSG,
                PostMessageW, RegisterClassW, SW_HIDE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
                TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_HOTKEY, WM_KEYDOWN,
                WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCLBUTTONDOWN,
                WNDCLASSW,
            },
        },
    },
    core::{BOOL, HSTRING, PWSTR},
};

pub const MAIN_WINDOW_TITLE: &str = "mstsc-mgr";
pub const FLOATING_WINDOW_TITLE: &str = "mstsc-mgr-floating";

const HOTKEY_NUM_BASE: i32 = 0x5100;
const HOTKEY_PREVIOUS: i32 = 0x5200;
const HOTKEY_NEXT: i32 = 0x5201;
const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const TRAY_ICON_ID: u32 = 1;

pub type WindowSnapshot = Arc<RwLock<Vec<MstscWindow>>>;
pub type RuntimeSettings = Arc<RwLock<AppSettings>>;

pub fn launch_connection(connection: &SavedConnection) -> Result<()> {
    if connection.host.trim().is_empty() {
        bail!("host is required");
    }
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

pub fn configure_floating_window_topmost() -> Result<()> {
    let hwnd = find_window_by_title(FLOATING_WINDOW_TITLE)?;
    // SAFETY: hwnd is the floating controller window; flags preserve its size and position and do
    // not activate it while promoting it to the topmost Z band.
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
        .context("SetWindowPos topmost failed")?;
    }
    Ok(())
}

pub fn resize_floating_window(width: i32, height: i32) -> Result<()> {
    let hwnd = find_window_by_title(FLOATING_WINDOW_TITLE)?;
    let mut rect = RECT::default();
    // SAFETY: hwnd is the floating controller; rect is valid writable storage. SetWindowPos keeps
    // the current right/top anchor while changing only the desired native popup bounds.
    unsafe {
        GetWindowRect(hwnd, &mut rect).context("GetWindowRect failed")?;
        let x = rect.right.saturating_sub(width);
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            rect.top,
            width,
            height,
            SWP_NOACTIVATE,
        )
        .context("SetWindowPos resize failed")?;
    }
    Ok(())
}

pub fn begin_floating_drag() -> Result<()> {
    let hwnd = find_window_by_title(FLOATING_WINDOW_TITLE)?;
    // SAFETY: hwnd is the floating controller. Releasing capture and sending a non-client caption
    // press delegates the drag loop to Windows, which GPUI 0.2.x does not implement on Windows.
    unsafe {
        let _ = ReleaseCapture();
        let _ = SendMessageW(
            hwnd,
            WM_NCLBUTTONDOWN,
            Some(WPARAM(HTCAPTION as usize)),
            Some(LPARAM(0)),
        );
    }
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
            _ => {}
        }
    }
    // SAFETY: unhandled messages are forwarded to the default window procedure for this valid HWND.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub fn start_window_watcher(snapshot: WindowSnapshot) {
    thread::spawn(move || {
        loop {
            if let Ok(current) = enumerate_mstsc_windows()
                && let Ok(mut guard) = snapshot.write()
            {
                *guard = current;
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
