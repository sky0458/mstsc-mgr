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
        floating::{self, FloatingController},
        platform,
        ui::{self, AppState, ManagerView},
    };
    use std::sync::{Arc, RwLock};

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();

    let state = Arc::new(RwLock::new(AppState::load()?));
    if let Ok(guard) = state.read() {
        platform::start_window_watcher(Arc::clone(&guard.windows));
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
        let floating_state = Arc::clone(&state);
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

        platform::start_tray_worker();

        if floating_enabled
            && let Err(error) = cx.open_window(floating::floating_window_options(cx), |window, cx| {
                window.set_window_title(platform::FLOATING_WINDOW_TITLE);
                cx.new(|cx| FloatingController::new(floating_state, cx))
            })
        {
            tracing::error!(%error, "failed to open floating controller");
        }

        cx.activate(true);
    });
    Ok(())
}
