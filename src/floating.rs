use crate::{config, platform, ui::AppState};
use anyhow::{Context as _, Result};
use gpui::{
    App, Bounds, Context, IntoElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement, Styled, Timer, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, point, prelude::*, px, rgb, size,
};
use gpui_component::v_flex;
use std::{
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, POINT, RECT},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
            WindowsAndMessaging::{
                FindWindowW, GetCursorPos, GetSystemMetrics, GetWindowRect, HWND_TOPMOST,
                IsWindowVisible, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
                SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
                SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow,
            },
        },
    },
    core::HSTRING,
};

const PANEL: u32 = 0x172033;
const TEXT: u32 = 0xe5e7eb;
const MUTED: u32 = 0x94a3b8;
const ACCENT: u32 = 0x38bdf8;

const FLOATING_BALL_SIZE: f32 = 64.0;
const FLOATING_LIST_WIDTH: f32 = 240.0;
const FLOATING_LIST_BASE_HEIGHT: f32 = 48.0;
const FLOATING_LIST_ROW_HEIGHT: f32 = 36.0;
const FLOATING_MAX_TABS: usize = 9;
const FLOATING_MENU_WIDTH: f32 = 180.0;
const FLOATING_MENU_ITEM_HEIGHT: f32 = 32.0;
const FLOATING_MENU_ITEM_COUNT: usize = 3;
const FLOATING_MENU_OUTER_PADDING: f32 = 8.0;
const FLOATING_MENU_GAP_HEIGHT: f32 = 4.0;
const FLOATING_MENU_GAP: i32 = 2;
const FLOATING_DEFAULT_EDGE_GAP: i32 = 24;
const POINTER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const POINTER_LEAVE_GRACE: Duration = Duration::from_millis(500);
const LIST_REFRESH_INTERVAL: Duration = Duration::from_millis(350);
const WINDOW_REFRESH_INTERVAL: Duration = Duration::from_millis(700);
const MENU_DISMISS_POLL_INTERVAL: Duration = Duration::from_millis(15);
const POSITION_PERSIST_DELAY: Duration = Duration::from_millis(30);

pub const FLOATING_MENU_WINDOW_TITLE: &str = "mstsc-mgr-floating-menu";

fn floating_list_height(window_count: usize) -> f32 {
    let visible_rows = window_count.clamp(1, FLOATING_MAX_TABS) as f32;
    FLOATING_LIST_BASE_HEIGHT + visible_rows * FLOATING_LIST_ROW_HEIGHT
}

fn floating_menu_height() -> f32 {
    let gaps = FLOATING_MENU_ITEM_COUNT.saturating_sub(1) as f32;
    FLOATING_MENU_OUTER_PADDING
        + FLOATING_MENU_ITEM_COUNT as f32 * FLOATING_MENU_ITEM_HEIGHT
        + gaps * FLOATING_MENU_GAP_HEIGHT
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

fn floating_enabled_from_state(state: &Arc<RwLock<AppState>>) -> bool {
    state
        .read()
        .ok()
        .and_then(|state| {
            state
                .runtime_settings
                .read()
                .ok()
                .map(|settings| settings.floating_controller)
        })
        .unwrap_or(true)
}

fn find_floating_window(title: &str) -> Result<HWND> {
    let title = HSTRING::from(title);
    // SAFETY: FindWindowW only reads the supplied title and returns an OS-owned HWND.
    unsafe { FindWindowW(None, &title).context("FindWindowW failed") }
}

fn point_in_rect(point: POINT, rect: RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

pub fn set_controller_visible(visible: bool) -> Result<()> {
    let ball = find_floating_window(platform::FLOATING_BALL_WINDOW_TITLE)?;
    // SAFETY: ball is the application's existing floating GPUI window. Only visibility and Z-order
    // change here; size, native region and position are deliberately left untouched.
    unsafe {
        if visible {
            let _ = ShowWindow(ball, SW_SHOWNOACTIVATE);
            SetWindowPos(
                ball,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .context("SetWindowPos shown floating ball failed")?;
        } else {
            let _ = ShowWindow(ball, SW_HIDE);
        }
    }
    if !visible {
        let _ = platform::set_floating_list_visible(false);
        let _ = set_context_menu_visible(false);
    }
    tracing::info!(visible, "floating controller visibility changed");
    Ok(())
}

fn configure_context_menu_window() -> Result<()> {
    let menu = find_floating_window(FLOATING_MENU_WINDOW_TITLE)?;
    let width = FLOATING_MENU_WIDTH.round() as i32;
    let height = floating_menu_height().round() as i32;
    // SAFETY: menu is this process's hidden/visible popup. The operation only forces its compact
    // native size and keeps its current position while avoiding activation.
    unsafe {
        SetWindowPos(
            menu,
            Some(HWND_TOPMOST),
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOACTIVATE,
        )
        .context("SetWindowPos floating menu size failed")?;
    }
    Ok(())
}

fn position_context_menu() -> Result<()> {
    let ball = find_floating_window(platform::FLOATING_BALL_WINDOW_TITLE)?;
    let menu = find_floating_window(FLOATING_MENU_WINDOW_TITLE)?;
    let mut ball_rect = RECT::default();
    let mut menu_rect = RECT::default();
    // SAFETY: both HWNDs belong to this process and the RECT values are writable output buffers.
    unsafe {
        GetWindowRect(ball, &mut ball_rect).context("GetWindowRect floating ball failed")?;
        GetWindowRect(menu, &mut menu_rect).context("GetWindowRect floating menu failed")?;
    }

    let menu_width = (menu_rect.right - menu_rect.left).max(1);
    let menu_height = (menu_rect.bottom - menu_rect.top).max(1);
    // SAFETY: GetSystemMetrics reads virtual-desktop geometry and has no pointer inputs.
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
    let max_x = virtual_right.saturating_sub(menu_width).max(virtual_left);
    let max_y = virtual_bottom.saturating_sub(menu_height).max(virtual_top);

    let left_x = ball_rect
        .left
        .saturating_sub(FLOATING_MENU_GAP)
        .saturating_sub(menu_width);
    let right_x = ball_rect.right.saturating_add(FLOATING_MENU_GAP);
    let x = if left_x >= virtual_left {
        left_x
    } else {
        right_x.clamp(virtual_left, max_x)
    };
    let y = ball_rect.top.clamp(virtual_top, max_y);

    // SAFETY: only the independent custom-menu popup is moved and promoted to the topmost band.
    unsafe {
        SetWindowPos(
            menu,
            Some(HWND_TOPMOST),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .context("SetWindowPos floating menu failed")?;
    }
    Ok(())
}

pub fn set_context_menu_visible(visible: bool) -> Result<()> {
    let menu = find_floating_window(FLOATING_MENU_WINDOW_TITLE)?;
    // SAFETY: menu is the application's independent GPUI popup and is only shown/hidden here.
    unsafe {
        if visible {
            configure_context_menu_window()?;
            position_context_menu()?;
            let _ = ShowWindow(menu, SW_SHOWNOACTIVATE);
            SetWindowPos(
                menu,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .context("SetWindowPos shown floating menu failed")?;
        } else {
            let _ = ShowWindow(menu, SW_HIDE);
        }
    }
    Ok(())
}

fn start_context_menu_dismiss_watcher() {
    thread::spawn(|| {
        let mut left_was_down = false;
        let mut right_was_down = false;
        loop {
            // SAFETY: GetAsyncKeyState only reads global mouse-button state.
            let left_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
            // SAFETY: GetAsyncKeyState only reads global mouse-button state.
            let right_down = unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) } < 0;
            let external_press = (left_down && !left_was_down) || (right_down && !right_was_down);
            left_was_down = left_down;
            right_was_down = right_down;

            if external_press {
                let Ok(menu) = find_floating_window(FLOATING_MENU_WINDOW_TITLE) else {
                    thread::sleep(MENU_DISMISS_POLL_INTERVAL);
                    continue;
                };
                // SAFETY: menu is owned by this process and visibility is read-only here.
                if unsafe { IsWindowVisible(menu).as_bool() } {
                    let mut cursor = POINT::default();
                    let mut menu_rect = RECT::default();
                    let mut ball_rect = RECT::default();
                    let ball = find_floating_window(platform::FLOATING_BALL_WINDOW_TITLE).ok();
                    // SAFETY: cursor/RECTs are writable output storage for live app windows.
                    let menu_hit = unsafe {
                        GetCursorPos(&mut cursor).is_ok()
                            && GetWindowRect(menu, &mut menu_rect).is_ok()
                            && point_in_rect(cursor, menu_rect)
                    };
                    let ball_hit = ball.is_some_and(|ball| {
                        // SAFETY: ball belongs to this process and ball_rect is writable storage.
                        unsafe {
                            GetWindowRect(ball, &mut ball_rect).is_ok()
                                && point_in_rect(cursor, ball_rect)
                        }
                    });
                    if !menu_hit && !ball_hit {
                        let _ = set_context_menu_visible(false);
                        tracing::debug!("floating context menu dismissed by external mouse click");
                    }
                }
            }
            thread::sleep(MENU_DISMISS_POLL_INTERVAL);
        }
    });
}

fn persist_floating_disabled(state: &Arc<RwLock<AppState>>) -> Result<()> {
    let mut app_state = state
        .write()
        .map_err(|_| anyhow::anyhow!("state lock poisoned"))?;
    app_state.settings.floating_controller = false;
    let next = app_state.settings.clone();
    if let Ok(mut runtime) = app_state.runtime_settings.write() {
        *runtime = next.clone();
    }
    config::save_settings(&app_state.paths, &next)?;
    drop(app_state);
    set_controller_visible(false)
}

fn persist_current_floating_position(state: &Arc<RwLock<AppState>>) -> Result<()> {
    let ball = find_floating_window(platform::FLOATING_BALL_WINDOW_TITLE)?;
    let mut rect = RECT::default();
    // SAFETY: ball is this process's floating popup and rect is writable output storage.
    unsafe {
        GetWindowRect(ball, &mut rect).context("GetWindowRect floating position failed")?;
    }

    let mut app_state = state
        .write()
        .map_err(|_| anyhow::anyhow!("state lock poisoned"))?;
    if app_state.settings.floating_ball_x == Some(rect.left)
        && app_state.settings.floating_ball_y == Some(rect.top)
    {
        return Ok(());
    }
    app_state.settings.floating_ball_x = Some(rect.left);
    app_state.settings.floating_ball_y = Some(rect.top);
    let next = app_state.settings.clone();
    if let Ok(mut runtime) = app_state.runtime_settings.write() {
        *runtime = next.clone();
    }
    config::save_settings(&app_state.paths, &next)?;
    tracing::info!(x = rect.left, y = rect.top, "floating ball position saved");
    Ok(())
}

fn persist_current_floating_position_after_drag(state: Arc<RwLock<AppState>>) {
    thread::spawn(move || {
        thread::sleep(POSITION_PERSIST_DELAY);
        if let Err(error) = persist_current_floating_position(&state) {
            tracing::warn!(%error, "failed to persist floating-ball position");
        }
    });
}

fn restore_floating_ball_position(state: &Arc<RwLock<AppState>>) -> Result<()> {
    let ball = find_floating_window(platform::FLOATING_BALL_WINDOW_TITLE)?;
    let mut rect = RECT::default();
    // SAFETY: ball is this process's floating popup and rect is writable output storage.
    unsafe {
        GetWindowRect(ball, &mut rect).context("GetWindowRect before position restore failed")?;
    }
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);

    // SAFETY: GetSystemMetrics reads screen geometry only.
    let (virtual_left, virtual_top, virtual_width, virtual_height, primary_width, primary_height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
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

    let saved = state
        .read()
        .ok()
        .map(|state| {
            (
                state.settings.floating_ball_x,
                state.settings.floating_ball_y,
            )
        })
        .unwrap_or((None, None));
    let saved_position = match saved {
        (Some(x), Some(y)) if x >= virtual_left && x <= max_x && y >= virtual_top && y <= max_y => {
            Some((x, y))
        }
        _ => None,
    };

    let default_x = primary_width
        .saturating_sub(width)
        .saturating_sub(FLOATING_DEFAULT_EDGE_GAP)
        .clamp(virtual_left, max_x);
    let default_y = (primary_height.saturating_sub(height) / 2).clamp(virtual_top, max_y);
    let (x, y) = saved_position.unwrap_or((default_x, default_y));

    // SAFETY: only the app-owned ball position/Z-order changes; its size and region stay intact.
    unsafe {
        SetWindowPos(
            ball,
            Some(HWND_TOPMOST),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .context("SetWindowPos floating position restore failed")?;
    }
    tracing::info!(
        x,
        y,
        restored = saved_position.is_some(),
        "floating ball startup position applied"
    );
    Ok(())
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
    controller_visible: bool,
}

impl FloatingBall {
    pub fn new(state: Arc<RwLock<AppState>>, cx: &mut Context<Self>) -> Self {
        let poll_state = Arc::clone(&state);
        let initial_opacity = opacity_from_state(&state);
        let initial_visible = floating_enabled_from_state(&state);
        cx.spawn(async move |weak, cx| {
            loop {
                Timer::after(POINTER_POLL_INTERVAL).await;
                let Some(entity) = weak.upgrade() else {
                    break;
                };
                let pointer_inside = platform::cursor_in_floating_controls().unwrap_or(false);
                let (controller_enabled, always_show, opacity) = poll_state
                    .read()
                    .ok()
                    .and_then(|state| {
                        state.runtime_settings.read().ok().map(|settings| {
                            (
                                settings.floating_controller,
                                settings.always_show_tabs && settings.floating_controller,
                                settings.floating_opacity(),
                            )
                        })
                    })
                    .unwrap_or((true, false, 0.5));

                if entity
                    .update(cx, move |view, cx| {
                        if controller_enabled != view.controller_visible
                            && set_controller_visible(controller_enabled).is_ok()
                        {
                            view.controller_visible = controller_enabled;
                        }

                        let now = Instant::now();
                        if pointer_inside && controller_enabled {
                            view.last_pointer_inside = now;
                        }
                        let keep_visible = view.list_visible
                            && now.duration_since(view.last_pointer_inside) < POINTER_LEAVE_GRACE;
                        let desired_visible =
                            controller_enabled && (always_show || pointer_inside || keep_visible);
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
            controller_visible: initial_visible,
        }
    }
}

impl Render for FloatingBall {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(platform::FLOATING_BALL_WINDOW_TITLE);
        if !self.native_ready && platform::configure_floating_ball_window().is_ok() {
            match restore_floating_ball_position(&self.state) {
                Ok(()) => self.native_ready = true,
                Err(error) => tracing::warn!(%error, "floating-ball position restore will retry"),
            }
        }

        let release_state = Arc::clone(&self.state);
        div()
            .id("floating-ball-v4")
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
                let _ = set_context_menu_visible(false);
                if let Err(error) = platform::begin_floating_drag() {
                    tracing::error!(%error, "failed to begin floating-ball drag");
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                persist_current_floating_position_after_drag(Arc::clone(&release_state));
                let _ = set_context_menu_visible(false);
                if let Err(error) = platform::handle_floating_ball_click() {
                    tracing::error!(%error, "failed to open main window from floating ball");
                }
            })
            .on_mouse_up(MouseButton::Right, |_, _, _| {
                if let Err(error) = set_context_menu_visible(true) {
                    tracing::error!(%error, "failed to show custom floating context menu");
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
                        .id(("mstsc-list-row-v4", index))
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

pub struct FloatingContextMenu {
    state: Arc<RwLock<AppState>>,
    native_ready: bool,
}

impl FloatingContextMenu {
    pub fn new(state: Arc<RwLock<AppState>>) -> Self {
        start_context_menu_dismiss_watcher();
        Self {
            state,
            native_ready: false,
        }
    }
}

impl Render for FloatingContextMenu {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(FLOATING_MENU_WINDOW_TITLE);
        if !self.native_ready {
            window.resize(size(px(FLOATING_MENU_WIDTH), px(floating_menu_height())));
            if configure_context_menu_window().is_ok() {
                self.native_ready = true;
            }
        }
        let close_state = Arc::clone(&self.state);

        v_flex()
            .size_full()
            .p_1()
            .gap_1()
            .rounded_lg()
            .bg(rgb(PANEL))
            .child(
                div()
                    .id("floating-menu-show-main")
                    .w_full()
                    .h(px(FLOATING_MENU_ITEM_HEIGHT))
                    .px_3()
                    .rounded_md()
                    .bg(rgb(0x0f172a))
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_click(|_, _, _| {
                        let _ = set_context_menu_visible(false);
                        if let Err(error) = platform::show_main_window() {
                            tracing::error!(%error, "failed to show main window from floating menu");
                        }
                    })
                    .child("Show main window"),
            )
            .child(
                div()
                    .id("floating-menu-close")
                    .w_full()
                    .h(px(FLOATING_MENU_ITEM_HEIGHT))
                    .px_3()
                    .rounded_md()
                    .bg(rgb(0x0f172a))
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_click(move |_, _, _| {
                        if let Err(error) = persist_floating_disabled(&close_state) {
                            tracing::error!(%error, "failed to close floating controller");
                        }
                    })
                    .child("Close floating controller"),
            )
            .child(
                div()
                    .id("floating-menu-exit")
                    .w_full()
                    .h(px(FLOATING_MENU_ITEM_HEIGHT))
                    .px_3()
                    .rounded_md()
                    .bg(rgb(0x0f172a))
                    .text_sm()
                    .text_color(rgb(0xfca5a5))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_click(|_, _, app| {
                        let _ = set_context_menu_visible(false);
                        app.quit();
                    })
                    .child("Exit"),
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
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

pub fn floating_ball_window_options_with_visibility(cx: &App, visible: bool) -> WindowOptions {
    let mut options = floating_ball_window_options(cx);
    options.show = visible;
    options
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

pub fn floating_context_menu_window_options(cx: &App) -> WindowOptions {
    let display_bounds = cx
        .primary_display()
        .map(|display| display.bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.))));
    let width = px(FLOATING_MENU_WIDTH);
    let height = px(floating_menu_height());
    let origin = point(
        display_bounds.origin.x + display_bounds.size.width - width - px(96.),
        display_bounds.origin.y + px(100.),
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
