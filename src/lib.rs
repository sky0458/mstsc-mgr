pub mod domain;

#[cfg(windows)]
pub mod config;
#[cfg(windows)]
pub mod crypto;
#[cfg(windows)]
pub mod floating;
#[cfg(windows)]
pub mod floating_position;
#[cfg(windows)]
pub mod logging;
#[cfg(windows)]
#[path = "platform.rs"]
mod platform_native;
#[cfg(windows)]
pub mod rdp_launch;
#[cfg(windows)]
pub mod platform {
    pub use crate::platform_native::{
        FLOATING_BALL_WINDOW_TITLE, FLOATING_LIST_WINDOW_TITLE, MAIN_WINDOW_TITLE, RuntimeSettings,
        WindowSnapshot, activate_window, begin_floating_drag, configure_floating_ball_window,
        configure_floating_list_window, cursor_in_floating_controls, enumerate_mstsc_windows,
        handle_floating_ball_click, hide_main_window, set_floating_list_visible, show_main_window,
        start_hotkey_worker, start_keepalive_worker, start_tray_worker, start_window_watcher,
        take_force_exit_requested,
    };
    pub use crate::rdp_launch::launch_connection;
}
#[cfg(windows)]
pub mod platform_actions;
#[cfg(windows)]
pub mod ui;
