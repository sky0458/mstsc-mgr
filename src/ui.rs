use crate::{
    crypto,
    model::{ConnectionProfile, ConnectionStore, SharedCredential},
    mstsc, storage,
};
use anyhow::{Context, Result, bail};
use std::{cell::RefCell, ffi::c_void};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::HBRUSH,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, ReleaseCapture},
            WindowsAndMessaging::{
                BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
                CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE, CBS_DROPDOWNLIST,
                CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOHSCROLL, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW, IsWindow,
                LB_ADDSTRING, LB_ERR, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, LBN_DBLCLK,
                LBS_NOTIFY, LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MSG, MessageBoxW,
                PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCCREATE,
                WM_NCDESTROY, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
                WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_TABSTOP,
                WS_VISIBLE, WS_VSCROLL,
            },
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
const ID_ACCOUNTS: i32 = 106;

const ID_NAME: i32 = 201;
const ID_HOST: i32 = 202;
const ID_PORT: i32 = 203;
const ID_USERNAME: i32 = 204;
const ID_PASSWORD: i32 = 205;
const ID_FULLSCREEN: i32 = 206;
const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;
const ID_CREDENTIAL: i32 = 209;

const BUTTON_CHECKED: usize = 1;
const COLOR_WINDOW_BRUSH: isize = 6;

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct AppState {
    store: ConnectionStore,
    list: HWND,
    status: HWND,
    accounts_button: HWND,
    editing_accounts: bool,
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
    credential: HWND,
    credential_choices: Vec<(Option<u64>, String)>,
    credential_mode: bool,
}

struct EditorResult {
    profile: ConnectionProfile,
    password_changed_to: Option<String>,
}

pub fn run() -> Result<()> {
    let store = storage::load()?;
    APP_STATE.with(|slot| {
        *slot.borrow_mut() = Some(AppState {
            store,
            list: HWND::default(),
            status: HWND::default(),
            accounts_button: HWND::default(),
            editing_accounts: false,
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
    let background = HBRUSH(COLOR_WINDOW_BRUSH as *mut c_void);
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
                ID_ACCOUNTS if notification == BN_CLICKED as i32 => {
                    APP_STATE.with(|slot| {
                        if let Some(state) = slot.borrow_mut().as_mut() {
                            state.editing_accounts = !state.editing_accounts;
                        }
                    });
                    refresh_list();
                }
                ID_LIST if notification == LBN_DBLCLK as i32 => connect_selected(hwnd),
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
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
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
    let accounts_button = create_button(parent, "Shared accounts", 556, 350, 160, ID_ACCOUNTS);
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
            state.accounts_button = accounts_button;
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

#[allow(clippy::too_many_arguments)]
unsafe fn create_control(
    ex_style: WINDOW_EX_STYLE,
    class_name: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
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

unsafe fn send_message(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> LRESULT {
    SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(lparam)))
}

fn refresh_list() {
    let status = APP_STATE.with(|slot| {
        let state_ref = slot.borrow();
        let Some(state) = state_ref.as_ref() else {
            return String::new();
        };
        let mut lines = Vec::new();
        if state.editing_accounts {
            for item in &state.store.credentials {
                lines.push(format!("{}    {}", item.name, item.username));
            }
        } else {
            for item in &state.store.connections {
                let user = item
                    .credential_id
                    .and_then(|id| state.store.credentials.iter().find(|item| item.id == id))
                    .map(|item| format!("{} (shared: {})", item.username, item.name))
                    .unwrap_or_else(|| item.username.clone());
                lines.push(format!("{}    {}    {}", item.name, item.endpoint(), user));
            }
        }
        // SAFETY: these controls belong to this thread; SendMessageW consumes the strings synchronously.
        unsafe {
            send_message(state.list, LB_RESETCONTENT, 0, 0);
            for line in &lines {
                let wide = wide_null(line);
                send_message(state.list, LB_ADDSTRING, 0, wide.as_ptr() as isize);
            }
            if !lines.is_empty() {
                send_message(state.list, LB_SETCURSEL, 0, 0);
            }
            let _ = SetWindowTextW(
                state.accounts_button,
                &HSTRING::from(if state.editing_accounts {
                    "Connections"
                } else {
                    "Shared accounts"
                }),
            );
        }
        format!(
            "{} saved {}",
            lines.len(),
            if state.editing_accounts {
                "shared account(s) — edit once for all linked connections"
            } else {
                "connection(s)"
            }
        )
    });
    set_status(&status);
}

fn selected_index() -> Option<usize> {
    APP_STATE.with(|slot| {
        let state_ref = slot.borrow();
        let state = state_ref.as_ref()?;
        let index = unsafe { send_message(state.list, LB_GETCURSEL, 0, 0).0 };
        if index == LB_ERR as isize {
            None
        } else {
            usize::try_from(index).ok()
        }
    })
}

fn update_store(parent: HWND, update: impl FnOnce(&mut ConnectionStore) -> Result<()>) {
    let result: Result<()> = APP_STATE.with(|slot| {
        let mut state_ref = slot.borrow_mut();
        let state = state_ref
            .as_mut()
            .context("application state unavailable")?;
        let mut next = state.store.clone();
        update(&mut next)?;
        storage::save(&next)?;
        state.store = next;
        Ok(())
    });
    // Drop the RefCell borrow before entering a nested MessageBox message loop.
    match result {
        Ok(()) => refresh_list(),
        Err(error) => show_error(Some(parent), &format!("Failed to save:\n{error:#}")),
    }
}

fn editing_accounts() -> bool {
    APP_STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|state| state.editing_accounts)
    })
}

fn add_connection(parent: HWND) {
    let accounts = editing_accounts();
    let next_id = APP_STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|state| {
                if accounts {
                    state.store.next_credential_id()
                } else {
                    state.store.next_id()
                }
            })
            .unwrap_or(1)
    });
    if let Some(result) = show_editor(parent, None, next_id, accounts) {
        update_store(parent, move |store| {
            if accounts {
                store.credentials.push(SharedCredential {
                    id: result.profile.id,
                    name: result.profile.name,
                    username: result.profile.username,
                    protected_password: result.profile.protected_password,
                });
            } else {
                store.connections.push(result.profile);
            }
            Ok(())
        });
    }
}

fn edit_connection(parent: HWND) {
    let Some(index) = selected_index() else {
        show_info(Some(parent), "Select an item first.");
        return;
    };
    let accounts = editing_accounts();
    let original = APP_STATE.with(|slot| {
        slot.borrow().as_ref().and_then(|state| {
            if accounts {
                state
                    .store
                    .credentials
                    .get(index)
                    .map(|item| ConnectionProfile {
                        id: item.id,
                        name: item.name.clone(),
                        host: String::new(),
                        port: 3389,
                        username: item.username.clone(),
                        protected_password: item.protected_password.clone(),
                        credential_id: None,
                        fullscreen: false,
                    })
            } else {
                state.store.connections.get(index).cloned()
            }
        })
    });
    let Some(original) = original else {
        return;
    };
    if let Some(result) = show_editor(parent, Some(original.clone()), original.id, accounts) {
        update_store(parent, move |store| {
            if accounts {
                let item = store
                    .credentials
                    .iter_mut()
                    .find(|item| item.id == original.id)
                    .context("shared account no longer exists")?;
                *item = SharedCredential {
                    id: result.profile.id,
                    name: result.profile.name,
                    username: result.profile.username,
                    protected_password: result.profile.protected_password,
                };
            } else {
                let item = store
                    .connections
                    .iter_mut()
                    .find(|item| item.id == original.id)
                    .context("connection no longer exists")?;
                *item = result.profile;
            }
            Ok(())
        });
    }
}

fn delete_connection(parent: HWND) {
    let Some(index) = selected_index() else {
        show_info(Some(parent), "Select an item first.");
        return;
    };
    let accounts = editing_accounts();
    update_store(parent, move |store| {
        if accounts {
            let id = store
                .credentials
                .get(index)
                .context("shared account no longer exists")?
                .id;
            store.remove_credential(id)?;
        } else if index < store.connections.len() {
            store.connections.remove(index);
        }
        Ok(())
    });
}

fn connect_selected(parent: HWND) {
    if editing_accounts() {
        show_info(Some(parent), "Select Connections to open a remote session.");
        return;
    }
    let Some(index) = selected_index() else {
        show_info(Some(parent), "Select a connection first.");
        return;
    };
    let result = APP_STATE
        .with(|slot| {
            let state_ref = slot.borrow();
            let state = state_ref
                .as_ref()
                .context("application state unavailable")?;
            let id = state
                .store
                .connections
                .get(index)
                .context("connection no longer exists")?
                .id;
            state.store.resolve_profile(id)
        })
        .and_then(|profile| {
            mstsc::launch(&profile)?;
            Ok(profile.session_title())
        });
    match result {
        Ok(title) => set_status(&format!("Launched {title}")),
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

fn show_editor(
    parent: HWND,
    original: Option<ConnectionProfile>,
    id: u64,
    credential_mode: bool,
) -> Option<EditorResult> {
    let mut choices = vec![(None, "Connection-specific account".to_string())];
    APP_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            choices.extend(
                state
                    .store
                    .credentials
                    .iter()
                    .map(|item| (Some(item.id), format!("{} — {}", item.name, item.username))),
            );
        }
    });
    let mut data = Box::new(EditorData {
        id,
        original,
        result: None,
        name: HWND::default(),
        host: HWND::default(),
        port: HWND::default(),
        username: HWND::default(),
        password: HWND::default(),
        fullscreen: HWND::default(),
        credential: HWND::default(),
        credential_choices: choices,
        credential_mode,
    });
    // SAFETY: the owned Box stays alive through creation, the modal message loop and destruction.
    // The main window is disabled to prevent nested editors from competing for capture/focus.
    let result = unsafe {
        (|| -> Result<()> {
            let instance = GetModuleHandleW(None).context("GetModuleHandleW failed")?;
            let _ = ReleaseCapture();
            let title = HSTRING::from(if credential_mode {
                "Shared account"
            } else {
                "Connection"
            });
            let hwnd = CreateWindowExW(
                WS_EX_DLGMODALFRAME,
                EDITOR_CLASS,
                &title,
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                480,
                if credential_mode { 320 } else { 420 },
                Some(parent),
                None,
                Some(HINSTANCE(instance.0)),
                Some((&mut *data as *mut EditorData).cast()),
            )
            .context("failed to create editor")?;
            let _ = EnableWindow(parent, false);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let mut message = MSG::default();
            let mut loop_failed = false;
            while IsWindow(Some(hwnd)).as_bool() {
                let received = GetMessageW(&mut message, None, 0, 0).0;
                if received <= 0 {
                    if received == 0 {
                        PostQuitMessage(message.wParam.0 as i32);
                    } else {
                        loop_failed = true;
                    }
                    break;
                }
                if !IsDialogMessageW(hwnd, &message).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if IsWindow(Some(hwnd)).as_bool() {
                let _ = DestroyWindow(hwnd);
            }
            let _ = ReleaseCapture();
            let _ = EnableWindow(parent, true);
            let _ = SetForegroundWindow(parent);
            if loop_failed {
                bail!("editor message loop failed");
            }
            Ok(())
        })()
    };
    if let Err(error) = result {
        show_error(Some(parent), &format!("Unable to open editor:\n{error:#}"));
        return None;
    }
    let mut result = data.result.take();
    if let Some(result_ref) = result.as_mut()
        && let Some(password) = result_ref.password_changed_to.take()
    {
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
            if id == ID_CREDENTIAL && notification == CBN_SELCHANGE as i32 {
                update_credential_controls(hwnd);
            }
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
        WM_NCDESTROY => {
            clear_editor_userdata(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
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
    let username = initial.as_ref().map(|p| p.username.as_str()).unwrap_or("");
    create_label(parent, "Name", 18, 22, 110);
    data.name = create_edit(parent, name, 136, 18, 306, ID_NAME, false);
    let (user_y, password_y, button_y) = if data.credential_mode {
        (62, 102, 218)
    } else {
        let host = initial.as_ref().map(|p| p.host.as_str()).unwrap_or("");
        let port = initial
            .as_ref()
            .map(|p| p.port.to_string())
            .unwrap_or_else(|| "3389".into());
        create_label(parent, "Host / IP", 18, 62, 110);
        data.host = create_edit(parent, host, 136, 58, 306, ID_HOST, false);
        create_label(parent, "Port", 18, 102, 110);
        data.port = create_edit(parent, &port, 136, 98, 100, ID_PORT, false);
        create_label(parent, "Account", 18, 142, 110);
        data.credential = create_control(
            WINDOW_EX_STYLE::default(),
            w!("COMBOBOX"),
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            136,
            138,
            306,
            200,
            parent,
            ID_CREDENTIAL,
        );
        let selected = initial.as_ref().and_then(|item| item.credential_id);
        for (index, (id, label)) in data.credential_choices.iter().enumerate() {
            let wide = wide_null(label);
            send_message(data.credential, CB_ADDSTRING, 0, wide.as_ptr() as isize);
            if *id == selected {
                send_message(data.credential, CB_SETCURSEL, index, 0);
            }
        }
        (182, 222, 328)
    };
    create_label(parent, "Username", 18, user_y, 110);
    data.username = create_edit(parent, username, 136, user_y - 4, 306, ID_USERNAME, false);
    create_label(parent, "Password", 18, password_y, 110);
    data.password = create_edit(parent, "", 136, password_y - 4, 306, ID_PASSWORD, true);
    create_label(
        parent,
        "Blank keeps existing password",
        136,
        password_y + 28,
        306,
    );
    if !data.credential_mode {
        data.fullscreen = create_control(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            "Full screen (/f)",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            136,
            286,
            240,
            24,
            parent,
            ID_FULLSCREEN,
        );
        if initial.as_ref().is_some_and(|p| p.fullscreen) {
            send_message(data.fullscreen, BM_SETCHECK, BUTTON_CHECKED, 0);
        }
    } else {
        create_label(
            parent,
            "Used by all linked connections on next connect",
            18,
            172,
            430,
        );
    }
    let _ = create_control(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        "Save",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        262,
        button_y,
        86,
        30,
        parent,
        ID_OK,
    );
    let _ = create_button(parent, "Cancel", 356, button_y, 86, ID_CANCEL);
    update_credential_controls(parent);
}

unsafe fn selected_credential(data: &EditorData) -> Option<u64> {
    if data.credential_mode {
        return None;
    }
    let index = send_message(data.credential, CB_GETCURSEL, 0, 0).0;
    usize::try_from(index)
        .ok()
        .and_then(|index| data.credential_choices.get(index))
        .and_then(|item| item.0)
}

unsafe fn update_credential_controls(hwnd: HWND) {
    if let Some(data) = editor_data_mut(hwnd) {
        let editable = data.credential_mode || selected_credential(data).is_none();
        let _ = EnableWindow(data.username, editable);
        let _ = EnableWindow(data.password, editable);
    }
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
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
    if password {
        style |= WINDOW_STYLE(windows::Win32::UI::WindowsAndMessaging::ES_PASSWORD as u32);
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
    let host = if data.credential_mode {
        String::new()
    } else {
        get_text(data.host).trim().to_string()
    };
    let port_text = if data.credential_mode {
        "3389".to_string()
    } else {
        get_text(data.port)
    };
    let credential_id = selected_credential(data);
    let username = get_text(data.username).trim().to_string();
    let password = if credential_id.is_none() {
        get_text(data.password)
    } else {
        String::new()
    };
    let fullscreen = send_message(data.fullscreen, BM_GETCHECK, 0, 0).0 == BUTTON_CHECKED as isize;

    if data.credential_mode && (name.is_empty() || username.is_empty()) {
        show_error(Some(hwnd), "Account name and username are required.");
        return;
    }
    if !data.credential_mode && host.is_empty() {
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
            username: if credential_id.is_none() {
                username
            } else {
                String::new()
            },
            protected_password: if credential_id.is_none() {
                protected_password
            } else {
                String::new()
            },
            credential_id,
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
