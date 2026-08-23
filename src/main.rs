#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("mstsc-mgr supports Windows 10 and newer only.");
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use gpui::{AppContext, Application};
    use gpui_component::Root;
    use gpui_component_assets::Assets;
    use mstsc_mgr::{
        floating::{self, FloatingBall, FloatingContextMenu, FloatingList},
        floating_position, logging, platform, platform_actions,
        ui::{self, AppState, ManagerView},
    };
    use std::sync::{Arc, RwLock};

    let state = Arc::new(RwLock::new(AppState::load()?));
    let log_path = state
        .read()
        .ok()
        .and_then(|guard| logging::init(Arc::clone(&guard.runtime_settings)));
    match log_path {
        Some(path) => tracing::info!(path = %path.display(), "diagnostic file logging initialized"),
        None => tracing::warn!("diagnostic log file could not be initialized"),
    }

    if let Ok(guard) = state.read() {
        floating::start_stable_window_watcher(Arc::clone(&guard.windows));
        platform::start_keepalive_worker(
            Arc::clone(&guard.windows),
            Arc::clone(&guard.runtime_settings),
        );
        platform::start_hotkey_worker(
            Arc::clone(&guard.windows),
            Arc::clone(&guard.runtime_settings),
        );
    }

    let app = Application::new().with_assets(Assets);
    app.run(move |cx| {
        gpui_component::init(cx);

        let manager_state = Arc::clone(&state);
        let floating_ball_state = Arc::clone(&state);
        let floating_list_state = Arc::clone(&state);
        let floating_menu_state = Arc::clone(&state);
        let floating_enabled = state
            .read()
            .map(|guard| guard.settings.floating_controller)
            .unwrap_or(true);

        if let Err(error) = cx.open_window(ui::main_window_options(cx), |window, cx| {
            window.set_window_title(platform::MAIN_WINDOW_TITLE);
            let close_state = Arc::clone(&manager_state);
            window.on_window_should_close(cx, move |_, app| {
                if platform::take_force_exit_requested() {
                    app.quit();
                    return true;
                }

                let close_to_tray = close_state
                    .read()
                    .map(|guard| guard.settings.close_to_tray)
                    .unwrap_or(true);
                if close_to_tray {
                    match platform::hide_main_window() {
                        Ok(()) => false,
                        Err(error) => {
                            tracing::error!(%error, "failed to hide main window to tray");
                            app.quit();
                            true
                        }
                    }
                } else {
                    app.quit();
                    true
                }
            });

            let view = cx.new(|_| ManagerView::new(Arc::clone(&manager_state)));
            cx.new(|cx| Root::new(view, window, cx))
        }) {
            tracing::error!(%error, "failed to open main window");
            cx.quit();
            return;
        }

        if let Err(error) = platform_actions::repair_main_window_frame() {
            tracing::warn!(%error, "failed to refresh native main-window frame after creation");
        }

        platform::start_tray_worker();

        if let Err(error) = cx.open_window(
            floating::floating_ball_window_options_with_visibility(cx, floating_enabled),
            |window, cx| {
                window.set_window_title(platform::FLOATING_BALL_WINDOW_TITLE);
                cx.new(|cx| FloatingBall::new(floating_ball_state, cx))
            },
        ) {
            tracing::error!(%error, "failed to open floating ball");
        }

        if let Err(error) = cx.open_window(
            floating::floating_list_window_options(cx),
            |window, cx| {
                window.set_window_title(platform::FLOATING_LIST_WINDOW_TITLE);
                cx.new(|cx| FloatingList::new(floating_list_state, cx))
            },
        ) {
            tracing::error!(%error, "failed to open floating MSTSC list");
        }

        if let Err(error) = cx.open_window(
            floating::floating_context_menu_window_options(cx),
            |window, cx| {
                window.set_window_title(floating::FLOATING_MENU_WINDOW_TITLE);
                cx.new(|_| FloatingContextMenu::new(floating_menu_state))
            },
        ) {
            tracing::error!(%error, "failed to open floating custom context menu");
        }

        if floating_enabled {
            if let Err(error) = platform::configure_floating_ball_window() {
                tracing::warn!(%error, "floating ball native configuration will retry during render");
            }
            if let Err(error) = platform::configure_floating_list_window() {
                tracing::warn!(%error, "floating list native configuration will retry during render");
            }
        }

        if let Err(error) = floating::set_controller_visible(floating_enabled) {
            tracing::warn!(%error, "failed to apply initial floating-controller visibility");
        }
        let initial_visible = floating::initial_list_visibility(&state);
        if let Err(error) = platform::set_floating_list_visible(initial_visible) {
            tracing::warn!(%error, "failed to apply initial floating-list visibility");
        }

        floating_position::start(Arc::clone(&state));

        cx.activate(true);
    });
    Ok(())
}
