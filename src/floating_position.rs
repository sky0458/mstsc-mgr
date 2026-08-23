use crate::{config, floating, platform, ui::AppState};
use anyhow::{Context as _, Result};
use std::{
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            WindowsAndMessaging::{
                FindWindowW, GetSystemMetrics, GetWindowRect, HWND_TOPMOST, SM_CXSCREEN,
                SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
                SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOSIZE, SetWindowPos,
            },
        },
    },
    core::HSTRING,
};

const STARTUP_DELAY: Duration = Duration::from_millis(120);
const STARTUP_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const STARTUP_RETRIES: usize = 20;
const POSITION_POLL_INTERVAL: Duration = Duration::from_millis(25);
const FLOATING_DEFAULT_EDGE_GAP: i32 = 24;
const FLOATING_DEFAULT_Y_PERCENT: i32 = 30;

pub fn start(state: Arc<RwLock<AppState>>, initially_visible: bool) {
    thread::spawn(move || {
        thread::sleep(STARTUP_DELAY);

        let mut positioned = false;
        for attempt in 1..=STARTUP_RETRIES {
            match apply_startup_position(&state) {
                Ok(()) => {
                    positioned = true;
                    tracing::info!(attempt, "floating ball startup position finalized");
                    break;
                }
                Err(error) => {
                    if attempt == STARTUP_RETRIES {
                        tracing::warn!(%error, "floating ball startup position could not be finalized");
                    } else {
                        tracing::debug!(attempt, %error, "floating ball startup position retry");
                        thread::sleep(STARTUP_RETRY_INTERVAL);
                    }
                }
            }
        }

        if positioned {
            if let Err(error) = floating::set_controller_visible(initially_visible) {
                tracing::warn!(%error, "failed to apply floating-controller visibility after positioning");
            }
            let list_visible = initially_visible && floating::initial_list_visibility(&state);
            if let Err(error) = platform::set_floating_list_visible(list_visible) {
                tracing::warn!(%error, "failed to apply floating-list visibility after positioning");
            }
        } else if initially_visible {
            if let Err(error) = floating::set_controller_visible(true) {
                tracing::warn!(%error, "failed to show floating controller after position fallback");
            }
        }

        watch_and_persist_drag_position(state);
    });
}

fn apply_startup_position(state: &Arc<RwLock<AppState>>) -> Result<()> {
    platform::configure_floating_ball_window()?;
    let ball = find_floating_ball()?;
    let mut rect = RECT::default();
    // SAFETY: ball is the application's live floating HWND and rect is writable output storage.
    unsafe {
        GetWindowRect(ball, &mut rect).context("GetWindowRect floating startup position failed")?;
    }

    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    // SAFETY: GetSystemMetrics reads process-independent desktop geometry.
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
        .map(|guard| {
            (
                guard.settings.floating_ball_x,
                guard.settings.floating_ball_y,
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
    let default_center_y = primary_height
        .saturating_mul(FLOATING_DEFAULT_Y_PERCENT)
        .saturating_div(100);
    let default_y = default_center_y
        .saturating_sub(height / 2)
        .clamp(virtual_top, max_y);
    let (x, y) = saved_position.unwrap_or((default_x, default_y));

    // SAFETY: only the app-owned floating HWND position/Z-order is changed; size is preserved.
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
        .context("SetWindowPos floating startup position failed")?;
    }
    tracing::info!(
        x,
        y,
        restored = saved_position.is_some(),
        "floating ball startup position applied after GPUI initialization"
    );
    Ok(())
}

fn watch_and_persist_drag_position(state: Arc<RwLock<AppState>>) {
    let mut last_position = read_floating_position().ok();
    let mut left_was_down = false;
    let mut moved_while_down = false;

    loop {
        // SAFETY: GetAsyncKeyState only reads the current global left-button state.
        let left_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
        let current_position = read_floating_position().ok();

        if left_down {
            if let (Some(previous), Some(current)) = (last_position, current_position)
                && previous != current
            {
                moved_while_down = true;
            }
        } else if left_was_down && moved_while_down {
            if let Some((x, y)) = current_position
                && let Err(error) = persist_position(&state, x, y)
            {
                tracing::warn!(%error, x, y, "failed to persist floating-ball drag position");
            }
            moved_while_down = false;
        }

        if current_position.is_some() {
            last_position = current_position;
        }
        left_was_down = left_down;
        thread::sleep(POSITION_POLL_INTERVAL);
    }
}

fn read_floating_position() -> Result<(i32, i32)> {
    let ball = find_floating_ball()?;
    let mut rect = RECT::default();
    // SAFETY: ball is the application's live floating HWND and rect is writable output storage.
    unsafe {
        GetWindowRect(ball, &mut rect).context("GetWindowRect floating position watcher failed")?;
    }
    Ok((rect.left, rect.top))
}

fn persist_position(state: &Arc<RwLock<AppState>>, x: i32, y: i32) -> Result<()> {
    let mut app_state = state
        .write()
        .map_err(|_| anyhow::anyhow!("state lock poisoned"))?;
    if app_state.settings.floating_ball_x == Some(x)
        && app_state.settings.floating_ball_y == Some(y)
    {
        return Ok(());
    }

    app_state.settings.floating_ball_x = Some(x);
    app_state.settings.floating_ball_y = Some(y);
    let next = app_state.settings.clone();
    if let Ok(mut runtime) = app_state.runtime_settings.write() {
        *runtime = next.clone();
    }
    config::save_settings(&app_state.paths, &next)?;
    tracing::info!(x, y, "floating ball position persisted after native drag");
    Ok(())
}

fn find_floating_ball() -> Result<HWND> {
    let title = HSTRING::from(platform::FLOATING_BALL_WINDOW_TITLE);
    // SAFETY: FindWindowW only reads the supplied title and returns an OS-owned HWND.
    unsafe { FindWindowW(None, &title).context("FindWindowW floating ball failed") }
}
