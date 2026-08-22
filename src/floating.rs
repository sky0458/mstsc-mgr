use crate::{platform, ui::AppState};
use anyhow::{Context as _, Result};
use gpui::{
    App, Bounds, Context, IntoElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement, Styled, Timer, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, point, prelude::*, px, rgb, size,
};
use gpui_component::v_flex;
use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, FindWindowW, GetCursorPos, HWND_TOPMOST,
            MF_SEPARATOR, MF_STRING, PostMessageW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
            SWP_NOMOVE, SetForegroundWindow, SetWindowPos, ShowWindow, TPM_LEFTALIGN,
            TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_CLOSE,
        },
    },
    core::HSTRING,
};

const PANEL: u32 = 0x172033;
const TEXT: u32 = 0xe5e7eb;
const MUTED: u32 = 0x94a3b8;
const ACCENT: u32 = 0x38bdf8;

const FLOATING_BALL_SIZE: f32 = 64.0;
const FLOATING_BALL_NATIVE_SIZE: i32 = 64;
const FLOATING_LIST_WIDTH: f32 = 240.0;
const FLOATING_LIST_BASE_HEIGHT: f32 = 48.0;
const FLOATING_LIST_ROW_HEIGHT: f32 = 36.0;
const FLOATING_MAX_TABS: usize = 9;
const POINTER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const POINTER_LEAVE_GRACE: Duration = Duration::from_millis(500);
const LIST_REFRESH_INTERVAL: Duration = Duration::from_millis(350);
const WINDOW_REFRESH_INTERVAL: Duration = Duration::from_millis(700);
const FLOATING_MENU_SHOW_MAIN: u16 = 2001;
const FLOATING_MENU_CLOSE: u16 = 2002;
const FLOATING_MENU_EXIT: u16 = 2003;

static FLOATING_CONTROLLER_VISIBLE: AtomicBool = AtomicBool::new(false);
static FLOATING_FORCE_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

fn floating_list_height(window_count: usize) -> f32 {
    let visible_rows = window_count.clamp(1, FLOATING_MAX_TABS) as f32;
    FLOATING_LIST_BASE_HEIGHT + visible_rows * FLOATING_LIST_ROW_HEIGHT
}

fn opacity_from_state(state: &Arc<RwLock<AppState>>) -> f32 {
    state
        .read()
        .ok()
        .and_then(|state| {
            state
                .runtime_settings
                .read()
                .ok()
                .map(|settings| settings.floating_opacity())
        })
        .unwrap_or(0.5)
}

fn find_window(title: &str) -> Result<HWND> {
    let title = HSTRING::from(title);
    // SAFETY: FindWindowW only reads the supplied title and returns a borrowed OS handle.
    unsafe { FindWindowW(None, &title).context("FindWindowW failed") }
}

pub fn set_controller_visible(visible: bool) -> Result<()> {
    FLOATING_CONTROLLER_VISIBLE.store(visible, Ordering::SeqCst);
    let ball = find_window(platform::FLOATING_BALL_WINDOW_TITLE)?;
    if visible {
        // SAFETY: before the hidden GPUI popup is shown, force the native HWND back to the exact
        // compact ball size. This prevents an early hidden-window layout from being used as the
        // ellipse size on affected Windows environments.
        unsafe {
            SetWindowPos(
                ball,
                Some(HWND_TOPMOST),
                0,
                0,
                FLOATING_BALL_NATIVE_SIZE,
                FLOATING_BALL_NATIVE_SIZE,
                SWP_NOMOVE | SWP_NOACTIVATE,
            )
            .context("SetWindowPos restore floating-ball size failed")?;
        }
        platform::configure_floating_ball_window()?;
        // SAFETY: ball is the application's own floating top-level window and is already sized and
        // configured before it becomes visible.
        unsafe {
            let _ = ShowWindow(ball, SW_SHOWNOACTIVATE);
        }
    } else {
        let _ = platform::set_floating_list_visible(false);
        // SAFETY: ball is the application's own floating top-level window.
        unsafe {
            let _ = ShowWindow(ball, SW_HIDE);
        }
    }
    tracing::info!(visible, "floating controller visibility changed");
    Ok(())
}

pub fn take_force_exit_requested() -> bool {
    FLOATING_FORCE_EXIT_REQUESTED.swap(false, Ordering::SeqCst)
}

fn request_application_exit() -> Result<()> {
    FLOATING_FORCE_EXIT_REQUESTED.store(true, Ordering::SeqCst);
    let main = find_window(platform::MAIN_WINDOW_TITLE)?;
    // SAFETY: main is the application's own top-level window. The main close callback sees the
    // floating force-exit flag and terminates instead of minimizing to the tray.
    unsafe {
        PostMessageW(Some(main), WM_CLOSE, WPARAM(0), LPARAM(0))
            .context("PostMessageW WM_CLOSE from floating menu failed")?;
    }
    Ok(())
}

fn show_floating_context_menu() -> Result<()> {
    let owner = find_window(platform::FLOATING_BALL_WINDOW_TITLE)?;
    // SAFETY: the popup menu is created/destroyed entirely on this dedicated worker thread.
    // TPM_NONOTIFY keeps menu command notifications out of the GPUI popup event loop, while
    // TPM_RETURNCMD lets this worker execute the selected action after TrackPopupMenu returns.
    unsafe {
        let menu = CreatePopupMenu().context("CreatePopupMenu for floating ball failed")?;
        let result = (|| -> Result<()> {
            AppendMenuW(
                menu,
                MF_STRING,
                usize::from(FLOATING_MENU_SHOW_MAIN),
                windows::core::w!("Show main window"),
            )
            .context("AppendMenuW floating show failed")?;
            AppendMenuW(
                menu,
                MF_STRING,
                usize::from(FLOATING_MENU_CLOSE),
                windows::core::w!("Close floating controller"),
            )
            .context("AppendMenuW floating close failed")?;
            AppendMenuW(menu, MF_SEPARATOR, 0, None)
                .context("AppendMenuW floating separator failed")?;
            AppendMenuW(
                menu,
                MF_STRING,
                usize::from(FLOATING_MENU_EXIT),
                windows::core::w!("Exit"),
            )
            .context("AppendMenuW floating exit failed")?;

            let mut cursor = POINT::default();
            GetCursorPos(&mut cursor).context("GetCursorPos for floating menu failed")?;
            let _ = SetForegroundWindow(owner);
            let command = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
                cursor.x,
                cursor.y,
                Some(0),
                owner,
                None,
            );
            match command.0 as u16 {
                FLOATING_MENU_SHOW_MAIN => platform::show_main_window()?,
                FLOATING_MENU_CLOSE => set_controller_visible(false)?,
                FLOATING_MENU_EXIT => request_application_exit()?,
                _ => {}
            }
            Ok(())
        })();
        let _ = DestroyMenu(menu);
        result
    }
}

fn show_floating_context_menu_async() {
    thread::spawn(|| {
        if let Err(error) = show_floating_context_menu() {
            tracing::error!(%error, "failed to show floating-ball context menu");
        }
    });
}

pub fn start_stable_window_watcher(snapshot: platform::WindowSnapshot) {
    thread::spawn(move || {
        let mut last_order: Vec<(u32, usize)> = Vec::new();
        loop {
            if let Ok(mut current) = platform::enumerate_mstsc_windows() {
                current.sort_by(|left, right| {
                    left.pid
                        .cmp(&right.pid)
                        .then_with(|| (left.hwnd as usize).cmp(&(right.hwnd as usize)))
                        .then_with(|| left.title.cmp(&right.title))
                });
                let order = current
                    .iter()
                    .map(|item| (item.pid, item.hwnd as usize))
                    .collect::<Vec<_>>();
                if order != last_order {
                    last_order = order;
                    tracing::info!(
                        count = current.len(),
                        "stable system-wide MSTSC window order changed"
                    );
                }
                if let Ok(mut guard) = snapshot.write() {
                    *guard = current;
                }
            }
            thread::sleep(WINDOW_REFRESH_INTERVAL);
        }
    });
}

pub struct FloatingBall {
    state: Arc<RwLock<AppState>>,
    list_visible: bool,
    last_pointer_inside: Instant,
    native_ready: bool,
    opacity: f32,
}

impl FloatingBall {
    pub fn new(state: Arc<RwLock<AppState>>, cx: &mut Context<Self>) -> Self {
        let poll_state = Arc::clone(&state);
        let initial_opacity = opacity_from_state(&state);
        cx.spawn(async move |weak, cx| {
            loop {
                Timer::after(POINTER_POLL_INTERVAL).await;
                let Some(entity) = weak.upgrade() else {
                    break;
                };
                let pointer_inside = platform::cursor_in_floating_controls().unwrap_or(false);
                let (always_show, opacity) = poll_state
                    .read()
                    .ok()
                    .and_then(|state| {
                        state.runtime_settings.read().ok().map(|settings| {
                            (
                                settings.always_show_tabs && settings.floating_controller,
                                settings.floating_opacity(),
                            )
                        })
                    })
                    .unwrap_or((false, 0.5));
                let controller_visible = FLOATING_CONTROLLER_VISIBLE.load(Ordering::SeqCst);

                if entity
                    .update(cx, move |view, cx| {
                        let now = Instant::now();
                        if pointer_inside && controller_visible {
                            view.last_pointer_inside = now;
                        }
                        let keep_visible = view.list_visible
                            && now.duration_since(view.last_pointer_inside) < POINTER_LEAVE_GRACE;
                        let desired_visible =
                            controller_visible && (always_show || pointer_inside || keep_visible);
                        if desired_visible != view.list_visible
                            && platform::set_floating_list_visible(desired_visible).is_ok()
                        {
                            view.list_visible = desired_visible;
                        }
                        if (view.opacity - opacity).abs() > f32::EPSILON {
                            view.opacity = opacity;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            state,
            list_visible: false,
            last_pointer_inside: Instant::now(),
            native_ready: false,
            opacity: initial_opacity,
        }
    }
}

impl Render for FloatingBall {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(platform::FLOATING_BALL_WINDOW_TITLE);
        window.resize(size(px(FLOATING_BALL_SIZE), px(FLOATING_BALL_SIZE)));
        if !self.native_ready && platform::configure_floating_ball_window().is_ok() {
            self.native_ready = true;
        }
        self.opacity = opacity_from_state(&self.state);

        div()
            .id("floating-ball-v6")
            .size_full()
            .rounded_full()
            .bg(rgb(ACCENT))
            .opacity(self.opacity)
            .text_color(rgb(0x082f49))
            .font_weight(gpui::FontWeight::BOLD)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, _| {
                if let Err(error) = platform::begin_floating_drag() {
                    tracing::error!(%error, "failed to begin floating-ball drag");
                }
            })
            .on_mouse_down(MouseButton::Right, |_, _, cx| {
                cx.stop_propagation();
                show_floating_context_menu_async();
            })
            .on_click(|event, _, _| {
                if event.down.button != MouseButton::Left {
                    return;
                }
                if let Err(error) = platform::handle_floating_ball_click() {
                    tracing::error!(%error, "failed to open main window from floating ball");
                }
            })
            .child("RDP")
    }
}

pub struct FloatingList {
    state: Arc<RwLock<AppState>>,
    native_ready: bool,
    last_height: Option<f32>,
}

impl FloatingList {
    pub fn new(state: Arc<RwLock<AppState>>, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |weak, cx| {
            loop {
                Timer::after(LIST_REFRESH_INTERVAL).await;
                let Some(entity) = weak.upgrade() else {
                    break;
                };
                if entity.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            state,
            native_ready: false,
            last_height: None,
        }
    }
}

impl Render for FloatingList {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(platform::FLOATING_LIST_WINDOW_TITLE);

        let windows = self
            .state
            .read()
            .ok()
            .and_then(|state| state.windows.read().ok().map(|windows| windows.clone()))
            .unwrap_or_default();
        let opacity = opacity_from_state(&self.state);
        let desired_height = floating_list_height(windows.len());
        if self.last_height != Some(desired_height) {
            window.resize(size(px(FLOATING_LIST_WIDTH), px(desired_height)));
            self.last_height = Some(desired_height);
            self.native_ready = false;
        }
        if !self.native_ready && platform::configure_floating_list_window().is_ok() {
            self.native_ready = true;
        }

        let mut rows = v_flex().gap_1().w_full();
        if windows.is_empty() {
            rows = rows.child(
                div()
                    .w_full()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x0f172a))
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("No MSTSC windows"),
            );
        } else {
            for (index, mstsc) in windows.iter().take(FLOATING_MAX_TABS).enumerate() {
                let hwnd = mstsc.hwnd;
                let label = format!("{}  {}", index + 1, mstsc.title);
                rows = rows.child(
                    div()
                        .id(("mstsc-list-row-v6", index))
                        .w_full()
                        .h(px(32.0))
                        .px_2()
                        .rounded_md()
                        .bg(rgb(0x0f172a))
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .on_click(move |_, _, _| {
                            if let Err(error) = platform::activate_window(hwnd) {
                                tracing::error!(%error, hwnd, "failed to activate MSTSC from floating list");
                            }
                        })
                        .child(label),
                );
            }
        }

        div().size_full().p_1().child(
            v_flex()
                .size_full()
                .p_2()
                .gap_1()
                .rounded_lg()
                .bg(rgb(PANEL))
                .opacity(opacity)
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child("MSTSC sessions"),
                )
                .child(rows),
        )
    }
}

pub fn floating_ball_window_options(cx: &App) -> WindowOptions {
    let display_bounds = cx
        .primary_display()
        .map(|display| display.bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.))));
    let diameter = px(FLOATING_BALL_SIZE);
    let origin = point(
        display_bounds.origin.x + display_bounds.size.width - diameter - px(24.),
        display_bounds.origin.y + px(100.),
    );

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            origin,
            size(diameter, diameter),
        ))),
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

pub fn floating_list_window_options(cx: &App) -> WindowOptions {
    let display_bounds = cx
        .primary_display()
        .map(|display| display.bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.))));
    let width = px(FLOATING_LIST_WIDTH);
    let height = px(floating_list_height(0));
    let origin = point(
        display_bounds.origin.x + display_bounds.size.width - width - px(24.),
        display_bounds.origin.y + px(100. + FLOATING_BALL_SIZE + 8.0),
    );

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            origin,
            size(width, height),
        ))),
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

pub fn initial_list_visibility(state: &Arc<RwLock<AppState>>) -> bool {
    state
        .read()
        .ok()
        .and_then(|state| {
            state
                .runtime_settings
                .read()
                .ok()
                .map(|settings| settings.floating_controller && settings.always_show_tabs)
        })
        .unwrap_or(false)
}
