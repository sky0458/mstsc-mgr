use crate::{
    config::{self, AppPaths},
    domain::{AppSettings, KeepAliveInput, SavedConnection, VaultPayload},
    platform::{self, RuntimeSettings, WindowSnapshot},
};
use gpui::{
    App, AppContext, Context, IntoElement, ParentElement, PathPromptOptions, Render, SharedString,
    Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_component::{
    Root, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement,
    v_flex,
};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

const BG: u32 = 0x0f172a;
const PANEL: u32 = 0x172033;
const TEXT: u32 = 0xe5e7eb;
const MUTED: u32 = 0x94a3b8;
const ACCENT: u32 = 0x38bdf8;

pub struct AppState {
    pub paths: AppPaths,
    pub settings: AppSettings,
    pub vault: VaultPayload,
    pub runtime_settings: RuntimeSettings,
    pub windows: WindowSnapshot,
}

impl AppState {
    pub fn load() -> anyhow::Result<Self> {
        let paths = AppPaths::discover()?;
        paths.ensure()?;
        let settings = config::load_settings(&paths).unwrap_or_default();
        let vault = config::load_vault(&paths).unwrap_or_default();
        Ok(Self {
            paths,
            runtime_settings: Arc::new(RwLock::new(settings.clone())),
            windows: Arc::new(RwLock::new(Vec::new())),
            settings,
            vault,
        })
    }
}

pub struct ManagerView {
    state: Arc<RwLock<AppState>>,
    status: SharedString,
}

impl ManagerView {
    pub fn new(state: Arc<RwLock<AppState>>) -> Self {
        Self {
            state,
            status: "Ready".into(),
        }
    }

    fn set_status(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = status.into();
        cx.notify();
    }

    fn open_connection_editor(
        &mut self,
        edit_id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .state
            .read()
            .ok()
            .and_then(|state| {
                edit_id.and_then(|id| {
                    state
                        .vault
                        .connections
                        .iter()
                        .find(|item| item.id == id)
                        .cloned()
                })
            })
            .unwrap_or_else(|| SavedConnection {
                id: 0,
                name: String::new(),
                host: String::new(),
                port: 3389,
                username: String::new(),
                password: String::new(),
                mstsc_args: Vec::new(),
            });

        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Display name")
                .default_value(existing.name.clone())
        });
        let host = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Host or IP")
                .default_value(existing.host.clone())
        });
        let port = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("3389")
                .default_value(existing.port.to_string())
        });
        let username = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("DOMAIN\\user or user")
                .default_value(existing.username.clone())
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .default_value(existing.password.clone())
                .masked(true)
        });
        let args = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Optional MSTSC args, separated by spaces (e.g. /f /multimon)")
                .default_value(existing.mstsc_args.join(" "))
        });

        let manager = cx.entity().clone();
        let state = Arc::clone(&self.state);
        window.open_dialog(cx, move |dialog, _, _| {
            let name = name.clone();
            let host = host.clone();
            let port = port.clone();
            let username = username.clone();
            let password = password.clone();
            let args = args.clone();
            let manager = manager.clone();
            let state = Arc::clone(&state);
            dialog
                .title(if edit_id.is_some() {
                    "Edit connection"
                } else {
                    "Add connection"
                })
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Save")
                        .cancel_text("Cancel"),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .child(field("Name", Input::new(&name)))
                        .child(field("Host", Input::new(&host)))
                        .child(field("Port", Input::new(&port)))
                        .child(field("Username", Input::new(&username)))
                        .child(field("Password", Input::new(&password)))
                        .child(field("MSTSC arguments", Input::new(&args))),
                )
                .on_ok(move |_, _, app| {
                    let name_value = name.read(app).value().to_string();
                    let host_value = host.read(app).value().to_string();
                    let port_value = port.read(app).value().to_string();
                    let username_value = username.read(app).value().to_string();
                    let password_value = password.read(app).value().to_string();
                    let args_value = args.read(app).value().to_string();
                    if host_value.trim().is_empty() {
                        manager.update(app, |view, cx| view.set_status("Host is required", cx));
                        return false;
                    }
                    let Ok(port_value) = port_value.trim().parse::<u16>() else {
                        manager.update(app, |view, cx| view.set_status("Port must be 1-65535", cx));
                        return false;
                    };
                    let save_result = state
                        .write()
                        .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                        .and_then(|mut app_state| {
                            let id = edit_id.unwrap_or_else(|| app_state.vault.next_id());
                            let item = SavedConnection {
                                id,
                                name: if name_value.trim().is_empty() {
                                    host_value.clone()
                                } else {
                                    name_value.trim().to_string()
                                },
                                host: host_value.trim().to_string(),
                                port: port_value,
                                username: username_value,
                                password: password_value,
                                mstsc_args: args_value
                                    .split_whitespace()
                                    .map(ToOwned::to_owned)
                                    .collect(),
                            };
                            if let Some(index) = app_state
                                .vault
                                .connections
                                .iter()
                                .position(|entry| entry.id == id)
                            {
                                app_state.vault.connections[index] = item;
                            } else {
                                app_state.vault.connections.push(item);
                            }
                            config::save_vault(&app_state.paths, &app_state.vault)
                        });
                    match save_result {
                        Ok(()) => {
                            manager.update(app, |view, cx| {
                                view.set_status("Connection saved securely", cx)
                            });
                            true
                        }
                        Err(error) => {
                            manager.update(app, |view, cx| {
                                view.set_status(format!("Save failed: {error:#}"), cx)
                            });
                            false
                        }
                    }
                })
        });
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self
            .state
            .read()
            .map(|state| state.settings.clone())
            .unwrap_or_default();
        let interval = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("60")
                .default_value(settings.keepalive_interval_seconds.to_string())
        });
        let draft = Arc::new(RwLock::new(settings));
        let manager = cx.entity().clone();
        let state = Arc::clone(&self.state);

        window.open_dialog(cx, move |dialog, _, _| {
            let draft_for_float = Arc::clone(&draft);
            let draft_for_tabs = Arc::clone(&draft);
            let draft_for_hotkeys = Arc::clone(&draft);
            let draft_for_tray = Arc::clone(&draft);
            let draft_for_logging = Arc::clone(&draft);
            let draft_for_keepalive = Arc::clone(&draft);
            let draft_for_input = Arc::clone(&draft);
            let draft_for_ok = Arc::clone(&draft);
            let interval_for_ok = interval.clone();
            let state_for_ok = Arc::clone(&state);
            let manager_for_ok = manager.clone();
            let current = draft.read().map(|v| v.clone()).unwrap_or_default();

            dialog
                .title("Settings")
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Save")
                        .cancel_text("Cancel"),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            Checkbox::new("floating-controller")
                                .label("Show floating controller")
                                .checked(current.floating_controller)
                                .on_click(move |checked, _, _| {
                                    if let Ok(mut value) = draft_for_float.write() {
                                        value.floating_controller = *checked;
                                    }
                                }),
                        )
                        .child(
                            Checkbox::new("always-tabs")
                                .label("Always show vertical MSTSC tabs")
                                .checked(current.always_show_tabs)
                                .on_click(move |checked, _, _| {
                                    if let Ok(mut value) = draft_for_tabs.write() {
                                        value.always_show_tabs = *checked;
                                    }
                                }),
                        )
                        .child(
                            Checkbox::new("global-hotkeys")
                                .label("Enable global hotkeys")
                                .checked(current.global_hotkeys)
                                .on_click(move |checked, _, _| {
                                    if let Ok(mut value) = draft_for_hotkeys.write() {
                                        value.global_hotkeys = *checked;
                                    }
                                }),
                        )
                        .child(
                            Checkbox::new("close-to-tray")
                                .label("Close main window to system tray")
                                .checked(current.close_to_tray)
                                .on_click(move |checked, _, _| {
                                    if let Ok(mut value) = draft_for_tray.write() {
                                        value.close_to_tray = *checked;
                                    }
                                }),
                        )
                        .child(
                            Checkbox::new("diagnostic-logging")
                                .label("Write diagnostic log file next to mstsc-mgr.exe")
                                .checked(current.logging_enabled)
                                .on_click(move |checked, _, _| {
                                    if let Ok(mut value) = draft_for_logging.write() {
                                        value.logging_enabled = *checked;
                                    }
                                }),
                        )
                        .child(
                            Checkbox::new("keepalive")
                                .label("Keep MSTSC sessions active")
                                .checked(current.keepalive_enabled)
                                .on_click(move |checked, _, _| {
                                    if let Ok(mut value) = draft_for_keepalive.write() {
                                        value.keepalive_enabled = *checked;
                                    }
                                }),
                        )
                        .child(field(
                            "Keepalive interval (seconds, minimum 5)",
                            Input::new(&interval),
                        ))
                        .child(
                            Checkbox::new("keepalive-key")
                                .label("Use Shift key event instead of mouse-move event")
                                .checked(current.keepalive_input == KeepAliveInput::ShiftKey)
                                .on_click(move |checked, _, _| {
                                    if let Ok(mut value) = draft_for_input.write() {
                                        value.keepalive_input = if *checked {
                                            KeepAliveInput::ShiftKey
                                        } else {
                                            KeepAliveInput::MouseMove
                                        };
                                    }
                                }),
                        ),
                )
                .on_ok(move |_, _, app| {
                    let interval_text = interval_for_ok.read(app).value().to_string();
                    let Ok(seconds) = interval_text.trim().parse::<u64>() else {
                        manager_for_ok.update(app, |view, cx| {
                            view.set_status("Keepalive interval must be a number", cx)
                        });
                        return false;
                    };
                    if seconds < 5 {
                        manager_for_ok.update(app, |view, cx| {
                            view.set_status("Keepalive interval must be at least 5 seconds", cx)
                        });
                        return false;
                    }
                    let mut next = draft_for_ok.read().map(|v| v.clone()).unwrap_or_default();
                    next.keepalive_interval_seconds = seconds;
                    let result = state_for_ok
                        .write()
                        .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                        .and_then(|mut app_state| {
                            app_state.settings = next.clone();
                            if let Ok(mut runtime) = app_state.runtime_settings.write() {
                                *runtime = next.clone();
                            }
                            config::save_settings(&app_state.paths, &next)
                        });
                    match result {
                        Ok(()) => {
                            manager_for_ok.update(app, |view, cx| {
                                view.set_status(
                                    "Settings saved; runtime switches apply immediately",
                                    cx,
                                )
                            });
                            true
                        }
                        Err(error) => {
                            manager_for_ok.update(app, |view, cx| {
                                view.set_status(format!("Settings save failed: {error:#}"), cx)
                            });
                            false
                        }
                    }
                })
        });
    }

    fn import_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chooser = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import encrypted MSTSC vault".into()),
        });
        let manager = cx.entity().clone();
        let state = Arc::clone(&self.state);
        window
            .spawn(cx, async move |cx| {
                let selected = chooser.await;
                let Ok(Ok(Some(paths))) = selected else {
                    return;
                };
                let Some(path) = paths.first().cloned() else {
                    return;
                };
                let result = config::import_vault(&path).and_then(|vault| {
                    let mut app_state = state
                        .write()
                        .map_err(|_| anyhow::anyhow!("state lock poisoned"))?;
                    app_state.vault = vault;
                    config::save_vault(&app_state.paths, &app_state.vault)
                });
                let _ = cx.update(|_, app| {
                    manager.update(app, |view, cx| match result {
                        Ok(()) => view.set_status(
                            format!("Imported encrypted vault from {}", path.display()),
                            cx,
                        ),
                        Err(error) => view.set_status(format!("Import failed: {error:#}"), cx),
                    });
                });
            })
            .detach();
    }

    fn export_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let directory = self
            .state
            .read()
            .map(|state| state.paths.root.clone())
            .unwrap_or_else(|_| PathBuf::from("."));
        let chooser = cx.prompt_for_new_path(&directory, Some("mstsc-mgr-vault.dpapi"));
        let manager = cx.entity().clone();
        let state = Arc::clone(&self.state);
        window
            .spawn(cx, async move |cx| {
                let selected = chooser.await;
                let Ok(Ok(Some(path))) = selected else {
                    return;
                };
                let result = state
                    .read()
                    .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                    .and_then(|app_state| config::export_vault(&app_state.vault, &path));
                let _ = cx.update(|_, app| {
                    manager.update(app, |view, cx| match result {
                        Ok(()) => view.set_status(
                            format!("Encrypted export saved to {}", path.display()),
                            cx,
                        ),
                        Err(error) => view.set_status(format!("Export failed: {error:#}"), cx),
                    });
                });
            })
            .detach();
    }
}

impl Render for ManagerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let connections = self
            .state
            .read()
            .map(|state| state.vault.connections.clone())
            .unwrap_or_default();

        let mut list = v_flex().gap_2();
        if connections.is_empty() {
            list = list.child(
                div()
                    .p_4()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .text_color(rgb(MUTED))
                    .child("No saved connections yet. Add one to start."),
            );
        }
        for connection in connections {
            let id = connection.id;
            let launch = connection.clone();
            let state_for_delete = Arc::clone(&self.state);
            list = list.child(
                h_flex()
                    .p_3()
                    .gap_3()
                    .items_center()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_color(rgb(TEXT))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(connection.name.clone()),
                            )
                            .child(div().text_sm().text_color(rgb(MUTED)).child(format!(
                                "{}  ·  {}",
                                connection.endpoint(),
                                connection.username
                            ))),
                    )
                    .child(
                        Button::new(("connect", id as usize))
                            .primary()
                            .label("Connect")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                match platform::launch_connection(&launch) {
                                    Ok(()) => {
                                        view.set_status(format!("Launching {}", launch.name), cx)
                                    }
                                    Err(error) => {
                                        view.set_status(format!("Launch failed: {error:#}"), cx)
                                    }
                                }
                            })),
                    )
                    .child(
                        Button::new(("edit", id as usize))
                            .label("Edit")
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.open_connection_editor(Some(id), window, cx)
                            })),
                    )
                    .child(
                        Button::new(("delete", id as usize))
                            .danger()
                            .label("Delete")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                let result = state_for_delete
                                    .write()
                                    .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                                    .and_then(|mut app_state| {
                                        app_state.vault.connections.retain(|item| item.id != id);
                                        config::save_vault(&app_state.paths, &app_state.vault)
                                    });
                                match result {
                                    Ok(()) => view.set_status("Connection deleted", cx),
                                    Err(error) => {
                                        view.set_status(format!("Delete failed: {error:#}"), cx)
                                    }
                                }
                            })),
                    ),
            );
        }

        div()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(
                v_flex()
                    .size_full()
                    .p_5()
                    .gap_4()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                v_flex()
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child("mstsc-mgr"),
                                    )
                                    .child(div().text_sm().text_color(rgb(MUTED)).child(
                                        "Native Rust + GPUI · RDM-style external MSTSC management",
                                    )),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("add")
                                            .primary()
                                            .label("Add connection")
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.open_connection_editor(None, window, cx)
                                            })),
                                    )
                                    .child(Button::new("settings").label("Settings").on_click(
                                        cx.listener(|view, _, window, cx| {
                                            view.open_settings(window, cx)
                                        }),
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("import")
                                    .label("Import encrypted vault")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.import_vault(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("export")
                                    .label("Export encrypted vault")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.export_vault(window, cx)
                                    })),
                            ),
                    )
                    .child(div().flex_1().overflow_y_scrollbar().child(list))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(ACCENT))
                            .child(self.status.clone()),
                    ),
            )
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

pub fn main_window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(860.), px(620.)), cx)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(platform::MAIN_WINDOW_TITLE.into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_min_size: Some(size(px(640.), px(480.))),
        ..Default::default()
    }
}

fn field(label: &'static str, input: Input) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_sm().text_color(rgb(MUTED)).child(label))
        .child(input)
}
