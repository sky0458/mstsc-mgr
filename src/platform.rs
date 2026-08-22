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
        Foundation::{CloseHandle, HWND, LPARAM, WPARAM},
        Security::Credentials::{
            CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
        },
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::{
            Input::KeyboardAndMouse::{
                MOD_ALT, MOD_CONTROL, MOD_SHIFT, RegisterHotKey, UnregisterHotKey, VIRTUAL_KEY,
                VK_LEFT, VK_RIGHT,
            },
            WindowsAndMessaging::{
                EnumWindows, GetForegroundWindow, GetMessageW, GetWindowTextLengthW,
                GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, MSG, PostMessageW,
                SW_RESTORE, SetForegroundWindow, ShowWindow, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP,
                WM_MOUSEMOVE,
            },
        },
    },
    core::{BOOL, PWSTR},
};

const HOTKEY_NUM_BASE: i32 = 0x5100;
const HOTKEY_PREVIOUS: i32 = 0x5200;
const HOTKEY_NEXT: i32 = 0x5201;

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

    let mut credential = CREDENTIALW {
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
    unsafe { CredWriteW(&mut credential, 0).context("CredWriteW failed")? };
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
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: HWND originated from EnumWindows. Calls are best-effort window state operations.
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        if SetForegroundWindow(hwnd).as_bool() {
            Ok(())
        } else {
            bail!("SetForegroundWindow was rejected by Windows foreground policy")
        }
    }
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
