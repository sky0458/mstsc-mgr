use crate::{domain::AppSettings, platform, ui::AppState};
use gpui::{
    App, Bounds, Context, IntoElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement, Styled, Timer, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, point, prelude::*, px, rgb, size,
};
use gpui_component::v_flex;
use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

const PANEL: u32 = 0x172033;
const TEXT: u32 = 0xe5e7eb;
const MUTED: u32 = 0x94a3b8;
const ACCENT: u32 = 0x38bdf8;

const FLOATING_PADDING: i32 = 10;
const FLOATING_BALL_SIZE: i32 = 52;
const FLOATING_COLLAPSED_SIZE: i32 = FLOATING_BALL_SIZE + FLOATING_PADDING * 2;
const FLOATING_EXPANDED_WIDTH: i32 = 380;
const FLOATING_TAB_HEIGHT: i32 = 44;
const FLOATING_MAX_TABS: usize = 9;
const POINTER_POLL_INTERVAL: Duration = Duration::from_millis(120);
const POINTER_LEAVE_GRACE: Duration = Duration::from_millis(360);

pub struct FloatingController {
    state: Arc<RwLock<AppState>>,
    expanded: bool,
    last_pointer_inside: Instant,
    native_ready: bool,
    last_native_size: Option<(i32, i32)>,
    refresh_tick: u8,
}

impl FloatingController {
    pub fn new(state: Arc<RwLock<AppState>>, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |weak, cx| {
            loop {
                Timer::after(POINTER_POLL_INTERVAL).await;
                let Some(entity) = weak.upgrade() else {
                    break;
                };
                let pointer_inside = platform::cursor_in_floating_window().unwrap_or(false);
                if entity
                    .update(cx, move |view, cx| {
                        let now = Instant::now();
                        let mut changed = false;

                        if pointer_inside {
                            view.last_pointer_inside = now;
                            if !view.expanded {
                                view.expanded = true;
                                changed = true;
                            }
                        } else if view.expanded
                            && now.duration_since(view.last_pointer_inside) >= POINTER_LEAVE_GRACE
                        {
                            view.expanded = false;
                            changed = true;
                        }

                        view.refresh_tick = view.refresh_tick.wrapping_add(1);
                        if changed || view.refresh_tick.is_multiple_of(4) {
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
            expanded: false,
            last_pointer_inside: Instant::now(),
            native_ready: false,
            last_native_size: None,
            refresh_tick: 0,
        }
    }
}

impl Render for FloatingController {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(platform::FLOATING_WINDOW_TITLE);

        let (settings, windows) = self
            .state
            .read()
            .map(|state| {
                let settings = state
                    .runtime_settings
                    .read()
                    .map(|value| value.clone())
                    .unwrap_or_default();
                let windows = state
                    .windows
                    .read()
                    .map(|value| value.clone())
                    .unwrap_or_default();
                (settings, windows)
            })
            .unwrap_or_else(|_| (AppSettings::default(), Vec::new()));

        let show_tabs = settings.always_show_tabs || self.expanded;
        let visible_rows = windows.len().clamp(1, FLOATING_MAX_TABS) as i32;
        let desired_size = if show_tabs {
            (
                FLOATING_EXPANDED_WIDTH,
                FLOATING_COLLAPSED_SIZE + 8 + visible_rows * FLOATING_TAB_HEIGHT,
            )
        } else {
            (FLOATING_COLLAPSED_SIZE, FLOATING_COLLAPSED_SIZE)
        };

        if !self.native_ready && platform::configure_floating_window_topmost().is_ok() {
            self.native_ready = true;
        }
        if self.last_native_size != Some(desired_size)
            && platform::resize_floating_window(desired_size.0, desired_size.1).is_ok()
        {
            self.last_native_size = Some(desired_size);
        }

        let mut tabs = v_flex().gap_1().mt_2().w_full().items_end();
        if show_tabs {
            if windows.is_empty() {
                tabs = tabs.child(
                    div()
                        .w(px(360.))
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(PANEL))
                        .opacity(0.94)
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("No MSTSC windows"),
                );
            }
            for (index, mstsc) in windows.iter().take(FLOATING_MAX_TABS).enumerate() {
                let hwnd = mstsc.hwnd;
                let label = format!("{}  {}", index + 1, mstsc.title);
                tabs = tabs.child(
                    div()
                        .id(("mstsc-tab-v2", index))
                        .w(px(360.))
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(PANEL))
                        .opacity(0.94)
                        .text_sm()
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .on_click(move |_, _, _| {
                            let _ = platform::activate_window(hwnd);
                        })
                        .child(label),
                );
            }
        }

        v_flex()
            .id("floating-controller-v2")
            .size_full()
            .p(px(FLOATING_PADDING as f32))
            .items_end()
            .child(
                div()
                    .id("floating-ball-v2")
                    .w(px(FLOATING_BALL_SIZE as f32))
                    .h(px(FLOATING_BALL_SIZE as f32))
                    .min_w(px(FLOATING_BALL_SIZE as f32))
                    .min_h(px(FLOATING_BALL_SIZE as f32))
                    .rounded_full()
                    .bg(rgb(ACCENT))
                    .text_color(rgb(0x082f49))
                    .font_weight(gpui::FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, _| {
                        let _ = platform::begin_floating_drag();
                    })
                    .child("RDP"),
            )
            .child(tabs)
    }
}

pub fn floating_window_options(cx: &App) -> WindowOptions {
    let display_bounds = cx
        .primary_display()
        .map(|display| display.bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.))));
    let width = px(FLOATING_COLLAPSED_SIZE as f32);
    let height = px(FLOATING_COLLAPSED_SIZE as f32);
    let origin = point(
        display_bounds.origin.x + display_bounds.size.width - width - px(24.),
        display_bounds.origin.y + px(100.),
    );

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            origin,
            size(width, height),
        ))),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}
