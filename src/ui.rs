use crate::{
    crypto,
    model::{ConnectionStore, SavedConnection, validate_fields},
    platform, storage,
};
use anyhow::{Context, Result, bail};
use std::{cell::RefCell, ffi::c_void};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::HBRUSH,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IDYES, LoadCursorW,
            MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_YESNO, MSG, MessageBoxW, PostQuitMessage,
            RegisterClassW, SW_SHOW, SendMessageW, SetWindowTextW, ShowWindow, TranslateMessage,
            WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_BORDER,
            WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
    core::{HSTRING, PCWSTR, w},
};

const WINDOW_CLASS: PCWSTR = w!("MstscMgrExternalWindow");

const ID_LIST: u16 = 100;
const ID_NAME: u16 = 101;
const ID_HOST: u16 = 102;
const ID_USERNAME: u16 = 103;
const ID_PASSWORD: u16 = 104;
const ID_NEW: u16 = 201;
const ID_SAVE: u16 = 202;
const ID_DELETE: u16 = 203;
const ID_CONNECT: u16 = 204;

const LBN_SELCHANGE_CODE: u16 = 1;
const LBN_DBLCLK_CODE: u16 = 2;
const LB_ADDSTRING_MSG: u32 = 0x0180;
const LB_RESETCONTENT_MSG: u32 = 0x0184;
const LB_SETCURSEL_MSG: u32 = 0x0186;
const LB_GETCURSEL_MSG: u32 = 0x0188;
const LBS_NOTIFY_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0001);
const ES_PASSWORD_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0020);
const ES_AUTOHSCROLL_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0080);

#[derive(Clone, Copy)]
struct UiHandles {
    main: HWND,
    list: HWND,
    name: HWND,
    host: HWND,
    username: HWND,
    password: HWND,
    status: HWND,
}

struct AppState {
    store: ConnectionStore,
    handles: Option<UiHandles>,
    selected: Option<usize>,
}

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

thread_local! {
    static APP: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

pub fn run() -> Result<()> {
    let store = storage::load()?;
    APP.with(|cell| {
        *cell.borrow_mut() = Some(AppState {
            store,
            handles: None,
            selected: None,
        });
    });

    // SAFETY: the registered class, instance and top-level HWND all live on this UI thread until
    // the message loop exits. All string pointers used here are static or owned for each call.
    unsafe {
        let module = GetModuleHandleW(None).context("GetModuleHandleW failed")?;
        let instance: HINSTANCE = module.into();
        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: cursor,
            hbrBackground: HBRUSH(6usize as *mut c_void),
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            bail!("RegisterClassW failed");
        }

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            w!("mstsc-mgr external"),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            120,
            120,
            760,
            430,
            None,
            None,
            Some(instance),
            None,
        )
        .context("CreateWindowExW main window failed")?;

        let handles = create_controls(hwnd, instance)?;
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.handles = Some(handles);
            }
        });
        refresh_list(None)?;
        set_status("就绪");

        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut message = MSG::default();
        loop {
            let code = GetMessageW(&mut message, None, 0, 0);
            if code.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    Ok(())
}

pub fn show_fatal_error(message: &str) {
    show_error(None, message);
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            if let Err(error) = handle_command(wparam) {
                show_error(Some(hwnd), &format!("{error:#}"));
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: hwnd is the live top-level application window supplied by the Win32 message
            // dispatcher. DestroyWindow ends its lifetime and triggers WM_DESTROY.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: this UI thread owns the message loop; posting quit is the normal shutdown path.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => {
            // SAFETY: unhandled messages for this valid HWND must be forwarded to DefWindowProcW.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

fn handle_command(wparam: WPARAM) -> Result<()> {
    let id = (wparam.0 & 0xffff) as u16;
    let code = ((wparam.0 >> 16) & 0xffff) as u16;

    match id {
        ID_LIST if code == LBN_SELCHANGE_CODE => on_list_selection(),
        ID_LIST if code == LBN_DBLCLK_CODE => {
            on_list_selection()?;
            on_connect()
        }
        ID_NEW if code == 0 => on_new(),
        ID_SAVE if code == 0 => on_save(),
        ID_DELETE if code == 0 => on_delete(),
        ID_CONNECT if code == 0 => on_connect(),
        _ => Ok(()),
    }
}

fn create_controls(parent: HWND, instance: HINSTANCE) -> Result<UiHandles> {
    create_control(
        parent,
        instance,
        w!("STATIC"),
        w!("已保存连接"),
        WS_CHILD | WS_VISIBLE,
        Rect {
            x: 20,
            y: 18,
            width: 300,
            height: 22,
        },
        0,
    )?;
    let list = create_control(
        parent,
        instance,
        w!("LISTBOX"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY_STYLE,
        Rect {
            x: 20,
            y: 44,
            width: 310,
            height: 300,
        },
        ID_LIST,
    )?;

    create_control(
        parent,
        instance,
        w!("STATIC"),
        w!("名称"),
        WS_CHILD | WS_VISIBLE,
        Rect {
            x: 360,
            y: 35,
            width: 90,
            height: 22,
        },
        0,
    )?;
    let name = create_control(
        parent,
        instance,
        w!("EDIT"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL_STYLE,
        Rect {
            x: 450,
            y: 30,
            width: 270,
            height: 26,
        },
        ID_NAME,
    )?;

    create_control(
        parent,
        instance,
        w!("STATIC"),
        w!("IP / 主机名"),
        WS_CHILD | WS_VISIBLE,
        Rect {
            x: 360,
            y: 82,
            width: 90,
            height: 22,
        },
        0,
    )?;
    let host = create_control(
        parent,
        instance,
        w!("EDIT"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL_STYLE,
        Rect {
            x: 450,
            y: 77,
            width: 270,
            height: 26,
        },
        ID_HOST,
    )?;

    create_control(
        parent,
        instance,
        w!("STATIC"),
        w!("用户名"),
        WS_CHILD | WS_VISIBLE,
        Rect {
            x: 360,
            y: 129,
            width: 90,
            height: 22,
        },
        0,
    )?;
    let username = create_control(
        parent,
        instance,
        w!("EDIT"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL_STYLE,
        Rect {
            x: 450,
            y: 124,
            width: 270,
            height: 26,
        },
        ID_USERNAME,
    )?;

    create_control(
        parent,
        instance,
        w!("STATIC"),
        w!("密码"),
        WS_CHILD | WS_VISIBLE,
        Rect {
            x: 360,
            y: 176,
            width: 90,
            height: 22,
        },
        0,
    )?;
    let password = create_control(
        parent,
        instance,
        w!("EDIT"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL_STYLE | ES_PASSWORD_STYLE,
        Rect {
            x: 450,
            y: 171,
            width: 270,
            height: 26,
        },
        ID_PASSWORD,
    )?;

    create_control(
        parent,
        instance,
        w!("BUTTON"),
        w!("新建"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        Rect {
            x: 360,
            y: 232,
            width: 80,
            height: 32,
        },
        ID_NEW,
    )?;
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        w!("保存"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        Rect {
            x: 450,
            y: 232,
            width: 80,
            height: 32,
        },
        ID_SAVE,
    )?;
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        w!("删除"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        Rect {
            x: 540,
            y: 232,
            width: 80,
            height: 32,
        },
        ID_DELETE,
    )?;
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        w!("连接"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        Rect {
            x: 630,
            y: 232,
            width: 90,
            height: 32,
        },
        ID_CONNECT,
    )?;

    create_control(
        parent,
        instance,
        w!("STATIC"),
        w!("密码说明：编辑已有连接时留空表示不修改。"),
        WS_CHILD | WS_VISIBLE,
        Rect {
            x: 360,
            y: 285,
            width: 360,
            height: 22,
        },
        0,
    )?;
    let status = create_control(
        parent,
        instance,
        w!("STATIC"),
        w!(""),
        WS_CHILD | WS_VISIBLE,
        Rect {
            x: 20,
            y: 360,
            width: 700,
            height: 24,
        },
        0,
    )?;

    Ok(UiHandles {
        main: parent,
        list,
        name,
        host,
        username,
        password,
        status,
    })
}

fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    text: PCWSTR,
    style: WINDOW_STYLE,
    rect: Rect,
    id: u16,
) -> Result<HWND> {
    let menu = if id == 0 {
        None
    } else {
        Some(HMENU(usize::from(id) as *mut c_void))
    };

    // SAFETY: parent and instance belong to this process. class/text are valid NUL-terminated
    // strings. Child controls are owned by parent and live until the top-level window is destroyed.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            text,
            style,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            Some(parent),
            menu,
            Some(instance),
            None,
        )
        .context("CreateWindowExW child control failed")
    }
}

fn on_new() -> Result<()> {
    APP.with(|cell| {
        let mut guard = cell.borrow_mut();
        let app = guard.as_mut().context("application state missing")?;
        app.selected = None;
        Ok::<(), anyhow::Error>(())
    })?;
    clear_form()?;
    set_list_selection(None)?;
    set_status("新建连接：填写信息后点击“保存”");
    Ok(())
}

fn on_list_selection() -> Result<()> {
    let index = current_list_selection()?.context("未选择连接")?;
    let (handles, connection) = APP.with(|cell| {
        let mut guard = cell.borrow_mut();
        let app = guard.as_mut().context("application state missing")?;
        let connection = app
            .store
            .connections
            .get(index)
            .cloned()
            .context("连接索引无效")?;
        app.selected = Some(index);
        let handles = app.handles.context("UI handles missing")?;
        Ok::<_, anyhow::Error>((handles, connection))
    })?;

    set_text(handles.name, &connection.name);
    set_text(handles.host, &connection.host);
    set_text(handles.username, &connection.username);
    set_text(handles.password, "");
    set_status("已载入连接；密码留空保存表示保持原密码");
    Ok(())
}

fn on_save() -> Result<()> {
    let handles = handles()?;
    let name = get_text(handles.name).trim().to_owned();
    let host = get_text(handles.host).trim().to_owned();
    let username = get_text(handles.username).trim().to_owned();
    let password = get_text(handles.password);
    validate_fields(&name, &host, &username)?;

    let (new_index, old_host) = APP.with(|cell| {
        let mut guard = cell.borrow_mut();
        let app = guard.as_mut().context("application state missing")?;
        let selected = app.selected;
        let encrypted = if password.is_empty() {
            let index = selected.context("新建连接时密码不能为空")?;
            app.store
                .connections
                .get(index)
                .map(|item| item.password_dpapi.clone())
                .context("连接索引无效")?
        } else {
            crypto::protect_password(&password)?
        };

        let connection = SavedConnection {
            name,
            host,
            username,
            password_dpapi: encrypted,
        };

        let (index, old_host) = if let Some(index) = selected {
            let existing = app.store.connections.get(index).context("连接索引无效")?;
            let old_host = Some(existing.host.clone());
            app.store.connections[index] = connection;
            (index, old_host)
        } else {
            let index = app.store.connections.len();
            app.store.connections.push(connection);
            (index, None)
        };
        app.selected = Some(index);
        storage::save(&app.store)?;
        Ok::<_, anyhow::Error>((index, old_host))
    })?;

    if let Some(previous) = old_host {
        let current_host = APP.with(|cell| {
            cell.borrow()
                .as_ref()
                .and_then(|app| app.store.connections.get(new_index))
                .map(|item| item.host.clone())
        });
        if current_host.as_deref() != Some(previous.as_str()) {
            platform::delete_credential(&previous);
        }
    }

    refresh_list(Some(new_index))?;
    set_status("连接已保存");
    Ok(())
}

fn on_delete() -> Result<()> {
    let (handles, index, connection) = APP.with(|cell| {
        let guard = cell.borrow();
        let app = guard.as_ref().context("application state missing")?;
        let index = app.selected.context("请先选择要删除的连接")?;
        let connection = app
            .store
            .connections
            .get(index)
            .cloned()
            .context("连接索引无效")?;
        Ok::<_, anyhow::Error>((
            app.handles.context("UI handles missing")?,
            index,
            connection,
        ))
    })?;

    if !confirm(
        handles.main,
        &format!("确定删除连接“{}”吗？", connection.name),
    ) {
        return Ok(());
    }

    APP.with(|cell| {
        let mut guard = cell.borrow_mut();
        let app = guard.as_mut().context("application state missing")?;
        if index >= app.store.connections.len() {
            bail!("连接索引无效");
        }
        app.store.connections.remove(index);
        app.selected = None;
        storage::save(&app.store)?;
        Ok::<(), anyhow::Error>(())
    })?;

    platform::delete_credential(&connection.host);
    refresh_list(None)?;
    clear_form()?;
    set_status("连接已删除");
    Ok(())
}

fn on_connect() -> Result<()> {
    let connection = APP.with(|cell| {
        let guard = cell.borrow();
        let app = guard.as_ref().context("application state missing")?;
        let index = app.selected.context("请先选择一个已保存连接")?;
        app.store
            .connections
            .get(index)
            .cloned()
            .context("连接索引无效")
    })?;

    platform::connect(&connection)?;
    set_status(&format!("已启动 MSTSC：{}", connection.host));
    Ok(())
}

fn refresh_list(select: Option<usize>) -> Result<()> {
    let (list, entries) = APP.with(|cell| {
        let guard = cell.borrow();
        let app = guard.as_ref().context("application state missing")?;
        let list = app.handles.context("UI handles missing")?.list;
        let entries = app
            .store
            .connections
            .iter()
            .map(|item| format!("{}  [{} @ {}]", item.name, item.username, item.host))
            .collect::<Vec<_>>();
        Ok::<_, anyhow::Error>((list, entries))
    })?;

    // SAFETY: list is a live LISTBOX child control. LB_* messages are synchronous and any UTF-16
    // pointer used by LB_ADDSTRING remains valid until SendMessageW returns.
    unsafe {
        let _ = SendMessageW(list, LB_RESETCONTENT_MSG, Some(WPARAM(0)), Some(LPARAM(0)));
        for entry in &entries {
            let wide = wide_null(entry);
            let _ = SendMessageW(
                list,
                LB_ADDSTRING_MSG,
                Some(WPARAM(0)),
                Some(LPARAM(wide.as_ptr() as isize)),
            );
        }
    }
    set_list_selection(select)?;
    Ok(())
}

fn set_list_selection(index: Option<usize>) -> Result<()> {
    let list = handles()?.list;
    let value = index.unwrap_or(usize::MAX);
    // SAFETY: list is a live LISTBOX child and LB_SETCURSEL does not retain any pointers.
    unsafe {
        let _ = SendMessageW(list, LB_SETCURSEL_MSG, Some(WPARAM(value)), Some(LPARAM(0)));
    }
    APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.selected = index;
        }
    });
    Ok(())
}

fn current_list_selection() -> Result<Option<usize>> {
    let list = handles()?.list;
    // SAFETY: list is a live LISTBOX child and LB_GETCURSEL only returns an integer index.
    let value = unsafe { SendMessageW(list, LB_GETCURSEL_MSG, Some(WPARAM(0)), Some(LPARAM(0))).0 };
    if value < 0 {
        Ok(None)
    } else {
        Ok(Some(value as usize))
    }
}

fn clear_form() -> Result<()> {
    let handles = handles()?;
    set_text(handles.name, "");
    set_text(handles.host, "");
    set_text(handles.username, "");
    set_text(handles.password, "");
    Ok(())
}

fn handles() -> Result<UiHandles> {
    APP.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|app| app.handles)
            .context("UI handles missing")
    })
}

fn set_status(message: &str) {
    if let Ok(handles) = handles() {
        set_text(handles.status, message);
    }
}

fn get_text(hwnd: HWND) -> String {
    // SAFETY: hwnd is a live EDIT control owned by this process. The allocated buffer is sized from
    // GetWindowTextLengthW and remains writable for GetWindowTextW.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied as usize])
    }
}

fn set_text(hwnd: HWND, value: &str) {
    let text = HSTRING::from(value);
    // SAFETY: hwnd is a live child control and HSTRING owns a valid UTF-16 buffer for the call.
    unsafe {
        let _ = SetWindowTextW(hwnd, &text);
    }
}

fn confirm(parent: HWND, message: &str) -> bool {
    let message = HSTRING::from(message);
    // SAFETY: parent is the live application window and HSTRING buffers remain valid for the
    // synchronous MessageBoxW call.
    unsafe {
        MessageBoxW(
            Some(parent),
            &message,
            w!("mstsc-mgr external"),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

fn show_error(parent: Option<HWND>, message: &str) {
    let message = HSTRING::from(message);
    // SAFETY: optional parent is either null or a live application HWND. HSTRING remains valid for
    // the synchronous MessageBoxW call.
    unsafe {
        let _ = MessageBoxW(
            parent,
            &message,
            w!("mstsc-mgr external"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
