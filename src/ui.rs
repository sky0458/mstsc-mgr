use crate::{
    crypto,
    model::{ConnectionProfile, ConnectionStore},
    mstsc, storage,
};
use anyhow::{Context, Result, bail};
use std::{cell::RefCell, ffi::c_void};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::HBRUSH,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BST_CHECKED,
            COLOR_WINDOW, CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
            DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, EnableWindow, GWLP_USERDATA,
            GetMessageW, GetSysColorBrush, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
            HMENU, IDC_ARROW, LB_ADDSTRING, LB_ERR, LB_GETCURSEL, LB_RESETCONTENT, LBN_DBLCLK,
            LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MSG, MessageBoxW,
            PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow,
            SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
            WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCCREATE, WNDCLASSW, WS_BORDER,
            WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_MINIMIZEBOX,
            WS_OVERLAPPED, WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
    core::{HSTRING, PCWSTR, w},
};

const MAIN_CLASS: PCWSTR = w!("MstscMgrExternalMain");
const EDITOR_CLASS: PCWSTR = w!("MstscMgrExternalEditor");
const ID_LIST: i32 = 100;
const ID_ADD: i32 = 101;
const ID_EDIT: i32 = 102;
const ID_DELETE: i32 = 103;
const ID_CONNECT: i32 = 104;
const ID_OPEN_DATA: i32 = 105;

const ID_NAME: i32 = 201;
const ID_HOST: i32 = 202;
const ID_PORT: i32 = 203;
const ID_USERNAME: i32 = 204;
const ID_PASSWORD: i32 = 205;
const ID_FULLSCREEN: i32 = 206;
const ID_OK: i32 = 207;
const ID_CANCEL: i32 = 208;

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct AppState {
    store: ConnectionStore,
    list: HWND,
    status: HWND,
}

struct EditorData {
    id: u64,
    original: Option<ConnectionProfile>,
    result: Option<EditorResult>,
    name: HWND,
    host: HWND,
    port: HWND,
    username: HWND,
    password: HWND,
    fullscreen: HWND,
}

struct EditorResult {
    profile: ConnectionProfile,
    password_changed_to: Option<String>,
}

pub fn run() -> Result<()> {
    let store = storage::load().unwrap_or_else(|error| {
        show_error(
            None,
            &format!("Failed to load saved connections:\n{error:#}"),
        );
        ConnectionStore::default()
    });
    APP_STATE.with(|slot| {
        *slot.borrow_mut() = Some(AppState {
            store,
            list: HWND::default(),
            status: HWND::default(),
        });
    });

    let instance = unsafe { GetModuleHandleW(None).context("GetModuleHandleW failed")? };
    register_classes(HINSTANCE(instance.0))?;

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            MAIN_CLASS,
            w!("mstsc-mgr external - Windows Server 2016"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            780,
            470,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
        .context("failed to create main window")?
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    refresh_list();

    let mut message = MSG::default();
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn register_classes(instance: HINSTANCE) -> Result<()> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW).context("LoadCursorW failed")? };
    let background: HBRUSH = unsafe { GetSysColorBrush(COLOR_WINDOW) };
    let main = WNDCLASSW {
        lpfnWndProc: Some(main_window_proc),
        hInstance: instance,
        hCursor: cursor,
        hbrBackground: background,
        lpszClassName: MAIN_CLASS,
        ..Default::default()
    };
    let editor = WNDCLASSW {
        lpfnWndProc: Some(editor_window_proc),
        hInstance: instance,
        hCursor: cursor,
        hbrBackground: background,
        lpszClassName: EDITOR_CLASS,
        ..Default::default()
    };
    unsafe {
        if RegisterClassW(&main) == 0 {
            bail!("RegisterClassW(main) failed");
        }
        if RegisterClassW(&editor) == 0 {
            bail!("RegisterClassW(editor) failed");
        }
    }
    Ok(())
}

unsafe extern "system" fn main_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            create_main_controls(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = low_word(wparam.0) as i32;
            let notification = high_word(wparam.0) as i32;
            match id {
                ID_ADD if notification == BN_CLICKED as i32 => add_connection(hwnd),
                ID_EDIT if notification == BN_CLICKED as i32 => edit_connection(hwnd),
                ID_DELETE if notification == BN_CLICKED as i32 => delete_connection(hwnd),
                ID_CONNECT if notification == BN_CLICKED as i32 => connect_selected(hwnd),
                ID_OPEN_DATA if notification == BN_CLICKED as i32 => open_data_folder(hwnd),
                ID_LIST if notification == LBN_DBLCLK => connect_selected(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn create_main_controls(parent: HWND) {
    let list = create_control(
        WS_EX_CLIENTEDGE,
        w!("LISTBOX"),
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL,
        16,
        16,
        732,
        320,
        parent,
        ID_LIST,
    );
    let _ = create_button(parent, "Add", 16, 350, 92, ID_ADD);
    let _ = create_button(parent, "Edit", 116, 350, 92, ID_EDIT);
    let _ = create_button(parent, "Delete", 216, 350, 92, ID_DELETE);
    let _ = create_button(parent, "Connect", 316, 350, 112, ID_CONNECT);
    let _ = create_button(parent, "Data folder", 436, 350, 112, ID_OPEN_DATA);
    let status = create_control(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        "Ready",
        WS_CHILD | WS_VISIBLE,
        16,
        392,
        720,
        24,
        parent,
        0,
    );
    APP_STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.list = list;
            state.status = status;
        }
    });
}

unsafe fn create_button(parent: HWND, text: &str, x: i32, y: i32, width: i32, id: i32) -> HWND {
    create_control(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        x,
        y,
        width,
        30,
        parent,
        id,
    )
}

unsafe fn create_control(
    ex_style: WINDOW_EX_STYLE,
    class_name: PCWSTR,
    text: &str,
    style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    id: i32,
) -> HWND {
    let text = HSTRING::from(text);
    CreateWindowExW(
        ex_style,
        class_name,
        &text,
        style,
        x,
        y,
        width,
        height,
        Some(parent),
        if id == 0 {
            None
        } else {
            Some(HMENU(id as isize as *mut c_void))
        },
        None,
        None,
    )
    .unwrap_or_default()
}

fn refresh_list() {
    let count = APP_STATE.with(|slot| {
        let state_ref = slot.borrow();
        let Some(state) = state_ref.as_ref() else {
            return 0;
        };
        unsafe {
            SendMessageW(state.list, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
            for item in &state.store.connections {
                let user = if item.username.trim().is_empty() {
                    "<prompt>"
                } else {
                    item.username.as_str()
                };
                let line = format!("{}    {}    {}", item.name, item.endpoint(), user);
                let wide = wide_null(&line);
                SendMessageW(
                    state.list,
                    LB_ADDSTRING,
                    WPARAM(0),
                    LPARAM(wide.as_ptr() as isize),
                );
            }
        }
        state.store.connections.len()
    });
    set_status(&format!("{count} saved connection(s)"));
}

fn selected_index() -> Option<usize> {
    APP_STATE.with(|slot| {
        let state_ref = slot.borrow();
        let state = state_ref.as_ref()?;
        let index = unsafe { SendMessageW(state.list, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
        if index == LB_ERR as isize {
            None
        } else {
            usize::try_from(index).ok()
        }
    })
}

fn add_connection(parent: HWND) {
    let next_id = APP_STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|state| state.store.next_id())
            .unwrap_or(1)
    });
    if let Some(result) = show_editor(parent, None, next_id) {
        APP_STATE.with(|slot| {
            let mut state_ref = slot.borrow_mut();
            if let Some(state) = state_ref.as_mut() {
                state.store.connections.push(result.profile);
                if let Err(error) = storage::save(&state.store) {
                    show_error(
                        Some(parent),
                        &format!("Failed to save connection:\n{error:#}"),
                    );
                }
            }
        });
        refresh_list();
    }
}

fn edit_connection(parent: HWND) {
    let Some(index) = selected_index() else {
        show_info(Some(parent), "Select a connection first.");
        return;
    };
    let original = APP_STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|state| state.store.connections.get(index).cloned())
    });
    let Some(original) = original else {
        return;
    };
    if let Some(result) = show_editor(parent, Some(original.clone()), original.id) {
        APP_STATE.with(|slot| {
            let mut state_ref = slot.borrow_mut();
            if let Some(state) = state_ref.as_mut()
                && let Some(target) = state.store.connections.get_mut(index)
            {
                *target = result.profile;
                if let Err(error) = storage::save(&state.store) {
                    show_error(
                        Some(parent),
                        &format!("Failed to save connection:\n{error:#}"),
                    );
                }
            }
        });
        refresh_list();
    }
}

fn delete_connection(parent: HWND) {
    let Some(index) = selected_index() else {
        show_info(Some(parent), "Select a connection first.");
        return;
    };
    APP_STATE.with(|slot| {
        let mut state_ref = slot.borrow_mut();
        if let Some(state) = state_ref.as_mut()
            && index < state.store.connections.len()
        {
            state.store.connections.remove(index);
            if let Err(error) = storage::save(&state.store) {
                show_error(
                    Some(parent),
                    &format!("Failed to save connection:\n{error:#}"),
                );
            }
        }
    });
    refresh_list();
}

fn connect_selected(parent: HWND) {
    let Some(index) = selected_index() else {
        show_info(Some(parent), "Select a connection first.");
        return;
    };
    let profile = APP_STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|state| state.store.connections.get(index).cloned())
    });
    let Some(profile) = profile else {
        return;
    };
    match mstsc::launch(&profile) {
        Ok(()) => set_status(&format!("Launched {}", profile.endpoint())),
        Err(error) => show_error(Some(parent), &format!("Failed to launch mstsc:\n{error:#}")),
    }
}

fn open_data_folder(parent: HWND) {
    match storage::store_path() {
        Ok(path) => {
            let Some(folder) = path.parent() else {
                return;
            };
            if let Err(error) = std::process::Command::new("explorer.exe")
                .arg(folder)
                .spawn()
            {
                show_error(
                    Some(parent),
                    &format!("Failed to open data folder:\n{error}"),
                );
            }
        }
        Err(error) => show_error(
            Some(parent),
            &format!("Failed to resolve data folder:\n{error:#}"),
        ),
    }
}

fn set_status(text: &str) {
    APP_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            unsafe {
                let _ = SetWindowTextW(state.status, &HSTRING::from(text));
            }
        }
    });
}

fn show_editor(parent: HWND, original: Option<ConnectionProfile>, id: u64) -> Option<EditorResult> {
    let data = Box::new(EditorData {
        id,
        original,
        result: None,
        name: HWND::default(),
        host: HWND::default(),
        port: HWND::default(),
        username: HWND::default(),
        password: HWND::default(),
        fullscreen: HWND::default(),
    });
    let raw = Box::into_raw(data);
    let instance = unsafe { GetModuleHandleW(None).ok()? };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            EDITOR_CLASS,
            w!("Connection"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            440,
            350,
            Some(parent),
            None,
            Some(HINSTANCE(instance.0)),
            Some(raw.cast()),
        )
        .ok()?
    };
    unsafe {
        let _ = EnableWindow(parent, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }

    let mut message = MSG::default();
    unsafe {
        while GetWindowLongPtrW(hwnd, GWLP_USERDATA) != 0 {
            if !GetMessageW(&mut message, None, 0, 0).as_bool() {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = EnableWindow(parent, true);
        let _ = SetForegroundWindow(parent);
    }

    let data = unsafe { Box::from_raw(raw) };
    let mut result = data.result;
    if let Some(result_ref) = result.as_mut() {
        if let Some(password) = result_ref.password_changed_to.take() {
            match crypto::protect_text(&password) {
                Ok(protected) => result_ref.profile.protected_password = protected,
                Err(error) => {
                    show_error(
                        Some(parent),
                        &format!("Failed to protect password:\n{error:#}"),
                    );
                    return None;
                }
            }
        }
    }
    result
}

unsafe extern "system" fn editor_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_CREATE => {
            create_editor_controls(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = low_word(wparam.0) as i32;
            let notification = high_word(wparam.0) as i32;
            if notification == BN_CLICKED as i32 {
                match id {
                    ID_OK => save_editor(hwnd),
                    ID_CANCEL => {
                        clear_editor_userdata(hwnd);
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            clear_editor_userdata(hwnd);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn create_editor_controls(parent: HWND) {
    let Some(data) = editor_data_mut(parent) else {
        return;
    };
    let initial = data.original.clone();
    let name = initial.as_ref().map(|p| p.name.as_str()).unwrap_or("");
    let host = initial.as_ref().map(|p| p.host.as_str()).unwrap_or("");
    let port = initial
        .as_ref()
        .map(|p| p.port.to_string())
        .unwrap_or_else(|| "3389".to_string());
    let username = initial.as_ref().map(|p| p.username.as_str()).unwrap_or("");

    create_label(parent, "Name", 18, 22, 110);
    data.name = create_edit(parent, name, 136, 18, 270, ID_NAME, false);
    create_label(parent, "Host / IP", 18, 62, 110);
    data.host = create_edit(parent, host, 136, 58, 270, ID_HOST, false);
    create_label(parent, "Port", 18, 102, 110);
    data.port = create_edit(parent, &port, 136, 98, 100, ID_PORT, false);
    create_label(parent, "Username", 18, 142, 110);
    data.username = create_edit(parent, username, 136, 138, 270, ID_USERNAME, false);
    create_label(parent, "Password", 18, 182, 110);
    data.password = create_edit(parent, "", 136, 178, 270, ID_PASSWORD, true);
    if initial.is_some() {
        create_label(
            parent,
            "Leave password blank to keep existing",
            136,
            209,
            270,
        );
    }
    data.fullscreen = create_control(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        "Full screen (/f)",
        WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX,
        136,
        232,
        180,
        24,
        parent,
        ID_FULLSCREEN,
    );
    if initial.as_ref().is_some_and(|p| p.fullscreen) {
        SendMessageW(
            data.fullscreen,
            BM_SETCHECK,
            WPARAM(BST_CHECKED.0 as usize),
            LPARAM(0),
        );
    }
    let _ = create_control(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        "Save",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON,
        226,
        274,
        86,
        30,
        parent,
        ID_OK,
    );
    let _ = create_button(parent, "Cancel", 320, 274, 86, ID_CANCEL);
}

unsafe fn create_label(parent: HWND, text: &str, x: i32, y: i32, width: i32) {
    let _ = create_control(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        text,
        WS_CHILD | WS_VISIBLE,
        x,
        y,
        width,
        24,
        parent,
        0,
    );
}

unsafe fn create_edit(
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    id: i32,
    password: bool,
) -> HWND {
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL;
    if password {
        style |= windows::Win32::UI::WindowsAndMessaging::ES_PASSWORD;
    }
    create_control(
        WS_EX_CLIENTEDGE,
        w!("EDIT"),
        text,
        style,
        x,
        y,
        width,
        26,
        parent,
        id,
    )
}

unsafe fn save_editor(hwnd: HWND) {
    let Some(data) = editor_data_mut(hwnd) else {
        return;
    };
    let name = get_text(data.name).trim().to_string();
    let host = get_text(data.host).trim().to_string();
    let port_text = get_text(data.port);
    let username = get_text(data.username).trim().to_string();
    let password = get_text(data.password);
    let fullscreen = SendMessageW(data.fullscreen, BM_GETCHECK, WPARAM(0), LPARAM(0)).0
        == BST_CHECKED.0 as isize;

    if host.is_empty() {
        show_error(Some(hwnd), "Host / IP is required.");
        return;
    }
    let port = match port_text.trim().parse::<u16>() {
        Ok(port) if port != 0 => port,
        _ => {
            show_error(Some(hwnd), "Port must be between 1 and 65535.");
            return;
        }
    };
    let id = data.original.as_ref().map(|p| p.id).unwrap_or(data.id);
    let protected_password = data
        .original
        .as_ref()
        .map(|p| p.protected_password.clone())
        .unwrap_or_default();
    data.result = Some(EditorResult {
        profile: ConnectionProfile {
            id,
            name: if name.is_empty() { host.clone() } else { name },
            host,
            port,
            username,
            protected_password,
            fullscreen,
        },
        password_changed_to: if password.is_empty() {
            None
        } else {
            Some(password)
        },
    });
    clear_editor_userdata(hwnd);
    let _ = DestroyWindow(hwnd);
}

unsafe fn editor_data_mut(hwnd: HWND) -> Option<&'static mut EditorData> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorData;
    ptr.as_mut()
}

unsafe fn clear_editor_userdata(hwnd: HWND) {
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
}

unsafe fn get_text(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; len as usize + 1];
    let copied = GetWindowTextW(hwnd, &mut buffer);
    String::from_utf16_lossy(&buffer[..copied as usize])
}

fn show_error(parent: Option<HWND>, text: &str) {
    unsafe {
        let _ = MessageBoxW(
            parent,
            &HSTRING::from(text),
            w!("mstsc-mgr external"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn show_info(parent: Option<HWND>, text: &str) {
    unsafe {
        let _ = MessageBoxW(
            parent,
            &HSTRING::from(text),
            w!("mstsc-mgr external"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn low_word(value: usize) -> u16 {
    (value & 0xffff) as u16
}

fn high_word(value: usize) -> u16 {
    ((value >> 16) & 0xffff) as u16
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
