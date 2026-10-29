use crate::{
    config::{self, AppPaths},
    domain::{AppSettings, KeepAliveInput, SavedConnection, SharedCredential, VaultPayload},
    platform::{self, RuntimeSettings, WindowSnapshot},
};
use gpui::{
    App, AppContext, Context, DispatchPhase, IntoElement, MouseButton, MouseMoveEvent,
    MouseUpEvent, ParentElement, PathPromptOptions, Pixels, Point, Render, SharedString, Styled,
    Window, WindowBounds, WindowControlArea, WindowOptions, canvas, div, point, prelude::*, px,
    rgb, size,
};
use gpui_component::{
    IndexPath, Root, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::{Dialog, DialogButtonProps},
    h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement,
    select::{Select, SelectItem, SelectState},
    slider::{Slider, SliderState},
    v_flex,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, RwLock},
};

const BG: u32 = 0x0f172a;
const PANEL: u32 = 0x172033;
const TEXT: u32 = 0xe5e7eb;
const MUTED: u32 = 0x94a3b8;
const ACCENT: u32 = 0x38bdf8;

#[derive(Clone)]
struct CredentialChoice {
    id: Option<u64>,
    label: SharedString,
}

impl SelectItem for CredentialChoice {
    type Value = Option<u64>;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &Self::Value {
        &self.id
    }
}

#[derive(Default)]
struct DialogDrag {
    offset: Point<Pixels>,
    pointer: Option<Point<Pixels>>,
}

fn movable_dialog(
    dialog: Dialog,
    title: &'static str,
    drag: Rc<RefCell<DialogDrag>>,
    window: &Window,
) -> Dialog {
    let offset = drag.borrow().offset;
    dialog
        .ml(offset.x)
        .margin_top(window.viewport_size().height / 10. + offset.y)
        .title(
            div()
                .id("dialog-drag-title")
                .w_full()
                .cursor_move()
                .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    drag.borrow_mut().pointer = Some(event.position);
                    cx.stop_propagation();
                })
                .child(title),
        )
}

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
        let settings = config::load_settings(&paths)?;
        let vault = config::load_vault(&paths)?;
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
    dialog_drag: Rc<RefCell<DialogDrag>>,
    show_credentials: bool,
}

impl ManagerView {
    pub fn new(state: Arc<RwLock<AppState>>) -> Self {
        Self {
            state,
            status: "Ready".into(),
            dialog_drag: Rc::default(),
            show_credentials: false,
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
                credential_id: None,
                mstsc_args: Vec::new(),
            });

        let mut choices = vec![CredentialChoice {
            id: None,
            label: "Connection-specific account".into(),
        }];
        if let Ok(state) = self.state.read() {
            choices.extend(state.vault.credentials.iter().map(|item| CredentialChoice {
                id: Some(item.id),
                label: format!("{} · {}", item.name, item.username).into(),
            }));
        }
        let selected = choices
            .iter()
            .position(|item| item.id == existing.credential_id)
            .map(|row| IndexPath::default().row(row));
        let credential = cx.new(|cx| SelectState::new(choices, selected, window, cx));

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
        *self.dialog_drag.borrow_mut() = DialogDrag::default();
        let drag = Rc::clone(&self.dialog_drag);
        window.open_dialog(cx, move |dialog, window, app| {
            let name = name.clone();
            let host = host.clone();
            let port = port.clone();
            let username = username.clone();
            let password = password.clone();
            let args = args.clone();
            let credential = credential.clone();
            let use_inline = credential
                .read(app)
                .selected_value()
                .is_none_or(Option::is_none);
            let manager = manager.clone();
            let state = Arc::clone(&state);
            movable_dialog(
                dialog,
                if edit_id.is_some() {
                    "Edit connection"
                } else {
                    "Add connection"
                },
                Rc::clone(&drag),
                window,
            )
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
                    .child(
                        v_flex()
                            .gap_1()
                            .child("Account")
                            .child(Select::new(&credential)),
                    )
                    .when(use_inline, |this| {
                        this.child(field("Username", Input::new(&username)))
                            .child(field("Password", Input::new(&password)))
                    })
                    .when(!use_inline, |this| {
                        this.child("Uses the latest username/password from Shared accounts.")
                    })
                    .child(field("MSTSC arguments", Input::new(&args))),
            )
            .on_ok(move |_, _, app| {
                let name_value = name.read(app).value().to_string();
                let host_value = host.read(app).value().to_string();
                let port_value = port.read(app).value().to_string();
                let username_value = username.read(app).value().to_string();
                let password_value = password.read(app).value().to_string();
                let args_value = args.read(app).value().to_string();
                let credential_id = credential.read(app).selected_value().copied().flatten();
                if host_value.trim().is_empty() {
                    manager.update(app, |view, cx| view.set_status("Host is required", cx));
                    return false;
                }
                let Some(port_value) = port_value
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                else {
                    manager.update(app, |view, cx| view.set_status("Port must be 1-65535", cx));
                    return false;
                };
                let save_result = state
                    .write()
                    .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                    .and_then(|mut app_state| {
                        let mut next = app_state.vault.clone();
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
                            username: if credential_id.is_none() {
                                username_value
                            } else {
                                String::new()
                            },
                            password: if credential_id.is_none() {
                                password_value
                            } else {
                                String::new()
                            },
                            credential_id,
                            mstsc_args: args_value
                                .split_whitespace()
                                .map(ToOwned::to_owned)
                                .collect(),
                        };
                        if let Some(index) =
                            next.connections.iter().position(|entry| entry.id == id)
                        {
                            next.connections[index] = item;
                        } else {
                            next.connections.push(item);
                        }
                        config::save_vault(&app_state.paths, &next)?;
                        app_state.vault = next;
                        Ok(())
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

    fn open_credential_editor(
        &mut self,
        edit_id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self.state.read().ok().and_then(|state| {
            state
                .vault
                .credentials
                .iter()
                .find(|item| Some(item.id) == edit_id)
                .cloned()
        });
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Account name")
                .default_value(
                    existing
                        .as_ref()
                        .map(|item| item.name.clone())
                        .unwrap_or_default(),
                )
        });
        let username = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("DOMAIN\\user or user")
                .default_value(
                    existing
                        .as_ref()
                        .map(|item| item.username.clone())
                        .unwrap_or_default(),
                )
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
                .default_value(
                    existing
                        .as_ref()
                        .map(|item| item.password.clone())
                        .unwrap_or_default(),
                )
        });
        let manager = cx.entity().clone();
        let state = Arc::clone(&self.state);
        *self.dialog_drag.borrow_mut() = DialogDrag::default();
        let drag = Rc::clone(&self.dialog_drag);
        window.open_dialog(cx, move |dialog, window, _| {
            let name = name.clone();
            let username = username.clone();
            let password = password.clone();
            let manager = manager.clone();
            let state = Arc::clone(&state);
            movable_dialog(dialog, "Shared account", Rc::clone(&drag), window)
                .confirm()
                .button_props(DialogButtonProps::default().ok_text("Save").cancel_text("Cancel"))
                .child(v_flex().gap_3()
                    .child(field("Account name", Input::new(&name)))
                    .child(field("Username", Input::new(&username)))
                    .child(field("Password", Input::new(&password)))
                    .child("All connections using this account use the updated credentials on their next connection."))
                .on_ok(move |_, _, app| {
                    let name = name.read(app).value().trim().to_string();
                    let username = username.read(app).value().trim().to_string();
                    let password = password.read(app).value().to_string();
                    let result = state.write().map_err(|_| anyhow::anyhow!("state lock poisoned"))
                        .and_then(|mut state| {
                            let mut next = state.vault.clone();
                            let id = edit_id.unwrap_or_else(|| next.next_credential_id());
                            let credential = SharedCredential { id, name, username, password };
                            if let Some(item) = next.credentials.iter_mut().find(|item| item.id == id) {
                                *item = credential;
                            } else {
                                next.credentials.push(credential);
                            }
                            config::save_vault(&state.paths, &next)?;
                            state.vault = next;
                            Ok(())
                        });
                    let saved = result.is_ok();
                    manager.update(app, |view, cx| match result {
                        Ok(()) => view.set_status("Shared account saved securely", cx),
                        Err(error) => view.set_status(format!("Save failed: {error:#}"), cx),
                    });
                    saved
                })
        });
    }

    fn credential_list(&self, cx: &mut Context<Self>) -> gpui::Div {
        let credentials = self
            .state
            .read()
            .map(|state| state.vault.credentials.clone())
            .unwrap_or_default();
        let mut list = v_flex()
            .gap_2()
            .child("Shared accounts · update a password once for all linked connections");
        for credential in credentials {
            let id = credential.id;
            list = list.child(
                h_flex()
                    .gap_3()
                    .p_3()
                    .bg(rgb(PANEL))
                    .rounded_lg()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(credential.name)
                            .child(credential.username),
                    )
                    .child(
                        Button::new(("edit-credential", id as usize))
                            .label("Edit")
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.open_credential_editor(Some(id), window, cx)
                            })),
                    )
                    .child(
                        Button::new(("delete-credential", id as usize))
                            .danger()
                            .label("Delete")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                let result = view
                                    .state
                                    .write()
                                    .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                                    .and_then(|mut state| {
                                        let mut next = state.vault.clone();
                                        next.remove_credential(id)?;
                                        config::save_vault(&state.paths, &next)?;
                                        state.vault = next;
                                        Ok(())
                                    });
                                match result {
                                    Ok(()) => view.set_status("Shared account deleted", cx),
                                    Err(error) => {
                                        view.set_status(format!("Delete failed: {error:#}"), cx)
                                    }
                                }
                            })),
                    ),
            );
        }
        list
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
        let opacity = cx.new(|_| {
            SliderState::new()
                .min(10.0)
                .max(100.0)
                .step(5.0)
                .default_value(f32::from(settings.floating_opacity_percent))
        });
        let draft = Arc::new(RwLock::new(settings));
        let manager = cx.entity().clone();
        let state = Arc::clone(&self.state);

        *self.dialog_drag.borrow_mut() = DialogDrag::default();
        let drag = Rc::clone(&self.dialog_drag);
        window.open_dialog(cx, move |dialog, window, _| {
            let draft_for_float = Arc::clone(&draft);
            let draft_for_tabs = Arc::clone(&draft);
            let draft_for_hotkeys = Arc::clone(&draft);
            let draft_for_tray = Arc::clone(&draft);
            let draft_for_logging = Arc::clone(&draft);
            let draft_for_keepalive = Arc::clone(&draft);
            let draft_for_input = Arc::clone(&draft);
            let draft_for_ok = Arc::clone(&draft);
            let interval_for_ok = interval.clone();
            let opacity_for_view = opacity.clone();
            let opacity_for_ok = opacity.clone();
            let state_for_ok = Arc::clone(&state);
            let manager_for_ok = manager.clone();
            let current = draft.read().map(|v| v.clone()).unwrap_or_default();

            movable_dialog(dialog, "Settings", Rc::clone(&drag), window)
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
                            v_flex()
                                .gap_1()
                                .child(
                                    div().text_sm().text_color(rgb(MUTED)).child(
                                        "Floating controller opacity (10-100%, default 50%)",
                                    ),
                                )
                                .child(Slider::new(&opacity_for_view)),
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
                    next.floating_opacity_percent = opacity_for_ok
                        .read(app)
                        .value()
                        .start()
                        .round()
                        .clamp(10.0, 100.0)
                        as u8;
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
                    config::save_vault(&app_state.paths, &vault)?;
                    app_state.vault = vault;
                    Ok(())
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
            let account_label = connection
                .credential_id
                .and_then(|id| {
                    self.state.read().ok().and_then(|state| {
                        state
                            .vault
                            .credentials
                            .iter()
                            .find(|item| item.id == id)
                            .map(|item| format!("{} · {}", item.name, item.username))
                    })
                })
                .unwrap_or_else(|| connection.username.clone());
            let state_for_delete = Arc::clone(&self.state);
            list =
                list.child(
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
                                    account_label
                                ))),
                        )
                        .child(
                            Button::new(("connect", id as usize))
                                .primary()
                                .label("Connect")
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    let result = view
                                        .state
                                        .read()
                                        .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                                        .and_then(|state| state.vault.resolve_connection(id))
                                        .and_then(|connection| {
                                            platform::launch_connection(&connection)
                                        });
                                    match result {
                                        Ok(()) => view.set_status("Launching connection", cx),
                                        Err(error) => {
                                            view.set_status(format!("Launch failed: {error:#}"), cx)
                                        }
                                    }
                                })),
                        )
                        .child(Button::new(("edit", id as usize)).label("Edit").on_click(
                            cx.listener(move |view, _, window, cx| {
                                view.open_connection_editor(Some(id), window, cx)
                            }),
                        ))
                        .child(
                            Button::new(("delete", id as usize))
                                .danger()
                                .label("Delete")
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    let result = state_for_delete
                                        .write()
                                        .map_err(|_| anyhow::anyhow!("state lock poisoned"))
                                        .and_then(|mut app_state| {
                                            let mut next = app_state.vault.clone();
                                            next.connections.retain(|item| item.id != id);
                                            config::save_vault(&app_state.paths, &next)?;
                                            app_state.vault = next;
                                            Ok(())
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

        if self.show_credentials {
            list = self.credential_list(cx);
        }
        let drag_listeners = Rc::clone(&self.dialog_drag);
        div()
            .child(
                canvas(
                    move |_, _, _| (),
                    move |_, _, window, _| {
                        let move_drag = Rc::clone(&drag_listeners);
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let mut drag = move_drag.borrow_mut();
                            if event.pressed_button != Some(MouseButton::Left) {
                                drag.pointer = None;
                            }
                            if let Some(previous) = drag.pointer {
                                let viewport = window.viewport_size();
                                let next = drag.offset + event.position - previous;
                                let max_x = ((viewport.width - px(480.)) / 2. - px(8.)).max(px(0.));
                                let min_y = px(8.) - viewport.height / 10.;
                                let max_y = (viewport.height * 0.9 - px(80.)).max(min_y);
                                drag.offset =
                                    point(next.x.clamp(-max_x, max_x), next.y.clamp(min_y, max_y));
                                drag.pointer = Some(event.position);
                                window.refresh();
                            }
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, _| {
                            if phase == DispatchPhase::Capture && event.button == MouseButton::Left
                            {
                                drag_listeners.borrow_mut().pointer = None;
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
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
                                    .window_control_area(WindowControlArea::Drag)
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
                                            .label(if self.show_credentials {
                                                "Add account"
                                            } else {
                                                "Add connection"
                                            })
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                if view.show_credentials {
                                                    view.open_credential_editor(None, window, cx)
                                                } else {
                                                    view.open_connection_editor(None, window, cx)
                                                }
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
                                Button::new("shared-accounts")
                                    .label(if self.show_credentials {
                                        "Connections"
                                    } else {
                                        "Shared accounts"
                                    })
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.show_credentials = !view.show_credentials;
                                        cx.notify();
                                    })),
                            )
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
