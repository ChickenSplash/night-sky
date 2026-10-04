//! Linux/Wayland-only prototype: GTK owns the toplevel and a GDK subsurface
//! hosts Bevy's swapchain. No render texture is read back or copied into HTML.
use crate::{actions::UiCommand, native_panel, ui_bridge::NativeCommands};
use bevy::{
    app::PluginsState,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardFocusLost, KeyboardInput},
        mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel},
    },
    prelude::*,
    window::{
        CursorEntered, CursorLeft, CursorMoved, PrimaryWindow, RawHandleWrapper,
        RawHandleWrapperHolder, WindowCreated, WindowResized, WindowWrapper,
    },
};
use glib::translate::ToGlibPtr;
use gtk::{gdk, gio, glib, prelude::*};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use webkit2gtk::{
    NavigationPolicyDecisionExt, PolicyDecisionExt, SecurityManagerExt, URIRequestExt,
    URISchemeRequestExt, UserContentManager, UserContentManagerExt, WebContext, WebContextExt,
    WebView, WebViewExt,
};

const URI: &str = "night-sky://ui/native-panel.html?layout=sidebar";
mod smoke;

#[link(name = "gdk-3")]
unsafe extern "C" {
    fn gdk_wayland_window_get_wl_surface(window: *mut gdk::ffi::GdkWindow) -> *mut c_void;
    fn gdk_wayland_display_get_wl_display(display: *mut gdk::ffi::GdkDisplay) -> *mut c_void;
}

/// These borrowed handles never outlive the GTK host. The custom runner drops
/// the entire Bevy app (including the non-pipelined renderer) before the GDK
/// subsurface or GTK display is destroyed. GTK calls stay on the main thread.
struct NativeHandle {
    surface: usize,
    display: usize,
    // RawHandleWrapper's Arc retains real GDK objects, including during unwind.
    // SendWrapper enforces that these GTK references are finally dropped on
    // their creating thread. This mode must not enable pipelined rendering.
    _keepalive: send_wrapper::SendWrapper<(gdk::Window, gdk::Display)>,
}

impl HasWindowHandle for NativeHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ptr = NonNull::new(self.surface as *mut c_void).ok_or(HandleError::Unavailable)?;
        // SAFETY: The runner's host retains the live GDK surface until App drops.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr)))
        })
    }
}
impl HasDisplayHandle for NativeHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let ptr = NonNull::new(self.display as *mut c_void).ok_or(HandleError::Unavailable)?;
        // SAFETY: Same host lifetime guarantee as the window handle above.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(ptr)))
        })
    }
}

#[derive(Default)]
struct InputQueue {
    keys: Vec<KeyboardInput>,
    buttons: Vec<MouseButtonInput>,
    cursor: Vec<CursorMoved>,
    wheel: Vec<MouseWheel>,
    enter: bool,
    leave: bool,
    lost_focus: bool,
    last_cursor: Option<Vec2>,
}

pub struct GtkHost {
    window: gtk::Window,
    viewport: gtk::DrawingArea,
    surface: gdk::Window,
    webview: WebView,
    entity: Entity,
    ready: Rc<Cell<bool>>,
    input: Rc<RefCell<InputQueue>>,
    commands: mpsc::Receiver<UiCommand>,
    smoke_result: Rc<Cell<Option<bool>>>,
}

impl GtkHost {
    pub fn new(app: &mut App) -> Result<Self, String> {
        // SAFETY: Called at process startup, before Bevy or GTK starts threads.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        gtk::init().map_err(|e| e.to_string())?;
        let display = gdk::Display::default().ok_or("No GTK display")?;
        if display.type_().name() != "GdkWaylandDisplay" {
            return Err("--gtk-html-ui requires a Wayland GTK display, not X11".into());
        }
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("Night Sky GTK prototype");
        window.set_default_size(1440, 900);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let viewport = gtk::DrawingArea::new();
        viewport.set_hexpand(true);
        viewport.set_vexpand(true);
        viewport.set_can_focus(true);
        viewport.add_events(
            gdk::EventMask::POINTER_MOTION_MASK
                | gdk::EventMask::BUTTON_PRESS_MASK
                | gdk::EventMask::BUTTON_RELEASE_MASK
                | gdk::EventMask::SCROLL_MASK
                | gdk::EventMask::SMOOTH_SCROLL_MASK
                | gdk::EventMask::ENTER_NOTIFY_MASK
                | gdk::EventMask::LEAVE_NOTIFY_MASK
                | gdk::EventMask::FOCUS_CHANGE_MASK
                | gdk::EventMask::KEY_PRESS_MASK
                | gdk::EventMask::KEY_RELEASE_MASK,
        );

        let (tx, commands) = mpsc::sync_channel(128);
        let ready = Rc::new(Cell::new(false));
        let smoke_result = Rc::new(Cell::new(None));
        let received_smoke = smoke_result.clone();
        let manager = UserContentManager::new();
        manager.register_script_message_handler("native");
        let received = ready.clone();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let smoke_root = root.clone();
        let focus_viewport = viewport.clone();
        manager.connect_script_message_received(Some("native"), move |_, result| {
            let Some(value) = result.js_value() else {
                return;
            };
            let text = value.to_string();
            if text == "ready" {
                received.set(true);
                return;
            }
            if text == "focus_viewport" {
                focus_viewport.grab_focus();
                return;
            }
            if let Some(report) = text.strip_prefix("smoke:") {
                let path = smoke_root.join("data/logs/gtk-panel-smoke.json");
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let _ = std::fs::write(path, report);
                eprintln!("GTK panel smoke: {report}");
                received_smoke.set(
                    serde_json::from_str::<serde_json::Value>(report)
                        .ok()
                        .and_then(|r| r["ok"].as_bool()),
                );
                return;
            }
            match serde_json::from_str::<UiCommand>(&text) {
                // GTK already allocates a separate viewport, not an overlay.
                Ok(UiCommand::Panels { .. }) => {}
                Ok(command) => {
                    if let Err(error) = tx.try_send(command) {
                        eprintln!("GTK UI command queue: {error}");
                    }
                }
                Err(error) => eprintln!("GTK UI rejected command: {error}: {text}"),
            }
        });
        if std::env::var("SPACE_NATIVE_PANEL_SMOKE").as_deref() == Ok("1") {
            manager.add_script(&webkit2gtk::UserScript::new(
                "window.nativeSmoke = true;",
                webkit2gtk::UserContentInjectedFrames::TopFrame,
                webkit2gtk::UserScriptInjectionTime::Start,
                &[],
                &[],
            ));
        }
        let context = WebContext::new_ephemeral();
        if let Some(security) = context.security_manager() {
            security.register_uri_scheme_as_secure("night-sky");
            security.register_uri_scheme_as_cors_enabled("night-sky");
        }
        context.register_uri_scheme("night-sky", move |request| {
            let path = request.path().unwrap_or_default();
            let path = path.split('?').next().unwrap_or_default();
            let Some((relative, mime)) = asset(path) else {
                request.finish_error(&mut glib::Error::new(
                    gio::IOErrorEnum::NotFound,
                    "Unknown UI asset",
                ));
                return;
            };
            match std::fs::read(root.join("web").join(relative)) {
                Ok(bytes) => {
                    let length = bytes.len() as i64;
                    let bytes = glib::Bytes::from_owned(bytes);
                    request.finish(
                        &gio::MemoryInputStream::from_bytes(&bytes),
                        length,
                        Some(mime),
                    );
                }
                Err(error) => request.finish_error(&mut glib::Error::new(
                    gio::IOErrorEnum::NotFound,
                    &error.to_string(),
                )),
            }
        });
        let webview = WebView::builder()
            .web_context(&context)
            .user_content_manager(&manager)
            .build();
        webview.set_size_request(420, -1);
        webview.connect_decide_policy(|_, decision, kind| {
            if matches!(
                kind,
                webkit2gtk::PolicyDecisionType::NavigationAction
                    | webkit2gtk::PolicyDecisionType::NewWindowAction
            ) && let Some(nav) = decision.downcast_ref::<webkit2gtk::NavigationPolicyDecision>()
            {
                let uri = nav
                    .navigation_action()
                    .and_then(|a| a.request())
                    .and_then(|r| r.uri());
                if uri.as_deref() != Some(URI) {
                    decision.ignore();
                    return true;
                }
            }
            false
        });
        webview.connect_load_failed(|_, _, uri, error| {
            eprintln!("GTK UI load failed: {uri}: {error}");
            false
        });
        webview.load_uri(URI);
        row.pack_start(&webview, false, false, 0);
        row.pack_start(&viewport, true, true, 0);
        window.add(&row);
        window.show_all();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let parent =
            gtk::prelude::WidgetExt::window(&window).ok_or("GTK toplevel is unrealized")?;
        let allocation = viewport.allocation();
        let (x, y) = viewport
            .translate_coordinates(&window, 0, 0)
            .unwrap_or((allocation.x(), allocation.y()));
        // A root-child subsurface has abs_x/y = 0 in GDK. Parenting it as a
        // normal child would double its Wayland position (x + abs_x).
        // set_transient_for below establishes its real compositor parent.
        let surface = gdk::Window::new(
            None,
            &gdk::WindowAttr {
                window_type: gdk::WindowType::Subsurface,
                x: Some(x),
                y: Some(y),
                width: allocation.width().max(1),
                height: allocation.height().max(1),
                ..Default::default()
            },
        );
        surface.set_transient_for(&parent);
        // Hit testing belongs to GTK's viewport widget, not the swapchain surface.
        surface.input_shape_combine_region(&gtk::cairo::Region::create(), 0, 0);
        surface.show();
        // SAFETY: Checked Wayland display, live GTK objects, main thread only.
        let (wl_surface, wl_display) = unsafe {
            (
                gdk_wayland_window_get_wl_surface(surface.to_glib_none().0),
                gdk_wayland_display_get_wl_display(display.to_glib_none().0),
            )
        };
        if wl_surface.is_null() || wl_display.is_null() {
            return Err("GTK did not create a native Wayland subsurface".into());
        }
        let handle = RawHandleWrapper::new(&WindowWrapper::new(NativeHandle {
            surface: wl_surface as usize,
            display: wl_display as usize,
            _keepalive: send_wrapper::SendWrapper::new((surface.clone(), display)),
        }))
        .map_err(|e| e.to_string())?;
        let mut bevy_window = Window {
            title: "Night Sky GTK prototype".into(),
            ..Default::default()
        };
        bevy_window
            .resolution
            .set_scale_factor_override(Some(viewport.scale_factor() as f32));
        bevy_window.resolution.set(
            allocation.width().max(1) as f32,
            allocation.height().max(1) as f32,
        );
        let entity = app
            .world_mut()
            .spawn((
                bevy_window,
                PrimaryWindow,
                handle.clone(),
                RawHandleWrapperHolder(Arc::new(Mutex::new(Some(handle)))),
            ))
            .id();
        let input = Rc::new(RefCell::new(InputQueue::default()));
        bind_input(&viewport, entity, &input);
        Ok(Self {
            window,
            viewport,
            surface,
            webview,
            entity,
            ready,
            input,
            commands,
            smoke_result,
        })
    }

    pub fn install(self, app: &mut App) {
        app.insert_resource(NativeCommands(Mutex::new(self.commands)));
        app.add_systems(Update, disable_canvas_ime);
        let host = HostRunner {
            window: self.window,
            viewport: self.viewport,
            surface: self.surface,
            webview: self.webview,
            entity: self.entity,
            ready: self.ready,
            input: self.input,
            smoke_result: self.smoke_result,
        };
        app.set_runner(move |app| host.run(app));
    }
}

struct HostRunner {
    window: gtk::Window,
    viewport: gtk::DrawingArea,
    surface: gdk::Window,
    webview: WebView,
    entity: Entity,
    ready: Rc<Cell<bool>>,
    input: Rc<RefCell<InputQueue>>,
    smoke_result: Rc<Cell<Option<bool>>>,
}

impl HostRunner {
    fn run(self, mut app: App) -> AppExit {
        let closing = Rc::new(Cell::new(false));
        let flag = closing.clone();
        let close_handler = self.window.connect_delete_event(move |_, _| {
            flag.set(true);
            glib::Propagation::Stop
        });
        while app.plugins_state() != PluginsState::Ready {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            bevy::tasks::tick_global_task_pools_on_main_thread();
            if closing.get() {
                // Even a close during plugin setup must respect handle lifetime.
                drop(app);
                self.surface.destroy();
                self.window.disconnect(close_handler);
                self.window.close();
                return AppExit::Success;
            }
        }
        app.finish();
        app.cleanup();
        app.world_mut().write_message(WindowCreated {
            window: self.entity,
        });
        let app = Rc::new(RefCell::new(app));
        let frame_app = app.clone();
        let viewport = self.viewport.clone();
        let surface = self.surface.clone();
        let window = self.window.clone();
        let webview = self.webview.clone();
        let ready = self.ready.clone();
        let input = self.input.clone();
        let entity = self.entity;
        let smoke_result = self.smoke_result.clone();
        let smoke = RefCell::new(
            (std::env::var("SPACE_NATIVE_PANEL_SMOKE").as_deref() == Ok("1"))
                .then(smoke::Smoke::new),
        );
        let last_size = Cell::new((0, 0, 0));
        let previous = RefCell::new(BTreeMap::new());
        let next_publish = Cell::new(Instant::now());
        // Let GTK schedule frames, so rendering cannot starve webview painting
        // or input dispatch with an always-ready timer.
        let source = self.viewport.add_tick_callback(move |_, _| {
            if closing.get() {
                gtk::main_quit();
                return glib::ControlFlow::Continue;
            }
            let mut app = frame_app.borrow_mut();
            let allocation = viewport.allocation();
            let size = (
                allocation.width(),
                allocation.height(),
                viewport.scale_factor(),
            );
            if size.0 <= 0 || size.1 <= 0 || !viewport.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            if size != last_size.get() {
                let (x, y) = viewport
                    .translate_coordinates(&window, 0, 0)
                    .unwrap_or((allocation.x(), allocation.y()));
                surface.move_resize(x, y, size.0, size.1);
                let mut bevy_window = app.world_mut().get_mut::<Window>(entity).unwrap();
                bevy_window
                    .resolution
                    .set_scale_factor_override(Some(size.2 as f32));
                bevy_window.resolution.set(size.0 as f32, size.1 as f32);
                app.world_mut().write_message(WindowResized {
                    window: entity,
                    width: size.0 as f32,
                    height: size.1 as f32,
                });
                last_size.set(size);
            }
            drain_input(app.world_mut(), entity, &mut input.borrow_mut());
            app.update();
            if let Some(smoke) = smoke.borrow_mut().as_mut() {
                smoke.step(&mut app, &window, &viewport, &webview, smoke_result.get());
            }
            if app.should_exit().is_some() {
                gtk::main_quit();
            }
            if ready.get() && Instant::now() >= next_publish.get() {
                next_publish.set(Instant::now() + Duration::from_millis(100));
                let mut previous = previous.borrow_mut();
                let latest = native_panel::snapshot();
                for (name, json) in &latest {
                    if previous.get(name) == Some(json) {
                        continue;
                    }
                    let script = format!(
                        "window.dispatchEvent(new CustomEvent({}, {{detail:{}}}));",
                        serde_json::to_string(name).unwrap(),
                        serde_json::to_string(json).unwrap()
                    );
                    webview.evaluate_javascript(
                        &script,
                        None,
                        None,
                        gio::Cancellable::NONE,
                        |result| {
                            if let Err(error) = result {
                                eprintln!("GTK UI event failed: {error}");
                            }
                        },
                    );
                }
                *previous = latest;
            }
            glib::ControlFlow::Continue
        });
        gtk::main();
        source.remove(); // Drops the timer's references to App before destroying surfaces.
        let exit = app.borrow_mut().should_exit().unwrap_or(AppExit::Success);
        // Do not rely only on variable drop order: no callback may retain App.
        let app = match Rc::try_unwrap(app) {
            Ok(app) => app.into_inner(),
            Err(_) => panic!("GTK frame callback still retains the renderer during shutdown"),
        };
        drop(app); // Renderer and all raw handles must drop before GTK windows.
        self.surface.destroy();
        self.window.disconnect(close_handler);
        self.window.close();
        if std::env::var("SPACE_NATIVE_PANEL_SMOKE").as_deref() == Ok("1") {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/logs/gtk-host-smoke.json");
            if let Ok(contents) = std::fs::read_to_string(&path)
                && let Ok(mut report) = serde_json::from_str::<serde_json::Value>(&contents)
            {
                report["clean_shutdown"] = serde_json::json!(true);
                let _ = std::fs::write(path, report.to_string());
            }
            eprintln!("GTK renderer and surfaces shut down cleanly");
        }
        exit
    }
}

fn disable_canvas_ime(mut settings: Query<&mut bevy_egui::EguiContextSettings>) {
    for mut settings in &mut settings {
        if settings.enable_ime {
            settings.enable_ime = false;
        }
    }
}

fn asset(path: &str) -> Option<(&'static str, &'static str)> {
    Some(match path {
        "/native-panel.html" => ("native-panel.html", "text/html"),
        "/ui/panels.js" => ("ui/panels.js", "text/javascript"),
        "/ui/bridge.js" => ("ui/bridge.js", "text/javascript"),
        "/ui/native-panel-smoke.js" => ("ui/native-panel-smoke.js", "text/javascript"),
        "/ui/panels.css" => ("ui/panels.css", "text/css"),
        _ => return None,
    })
}

fn bind_input(viewport: &gtk::DrawingArea, entity: Entity, input: &Rc<RefCell<InputQueue>>) {
    for pressed in [true, false] {
        let queue = input.clone();
        let callback = move |_: &gtk::DrawingArea, event: &gdk::EventKey| {
            if let Some(key_code) = physical_key(event.hardware_keycode()) {
                queue.borrow_mut().keys.push(KeyboardInput {
                    key_code,
                    logical_key: event.keyval().to_unicode().map_or(
                        Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                        |c| Key::Character(c.to_string().into()),
                    ),
                    state: if pressed {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                    text: None,
                    repeat: false,
                    window: entity,
                });
            }
            glib::Propagation::Stop
        };
        if pressed {
            viewport.connect_key_press_event(callback);
        } else {
            viewport.connect_key_release_event(callback);
        }
    }
    for pressed in [true, false] {
        let queue = input.clone();
        let callback = move |area: &gtk::DrawingArea, event: &gdk::EventButton| {
            if pressed {
                area.grab_focus();
            }
            let button = match event.button() {
                1 => MouseButton::Left,
                2 => MouseButton::Middle,
                3 => MouseButton::Right,
                n => MouseButton::Other(n as u16),
            };
            queue.borrow_mut().buttons.push(MouseButtonInput {
                button,
                state: if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
                window: entity,
            });
            glib::Propagation::Stop
        };
        if pressed {
            viewport.connect_button_press_event(callback);
        } else {
            viewport.connect_button_release_event(callback);
        }
    }
    let queue = input.clone();
    viewport.connect_motion_notify_event(move |_, event| {
        let (x, y) = event.position();
        let position = Vec2::new(x as f32, y as f32);
        let mut queue = queue.borrow_mut();
        let delta = queue.last_cursor.map(|last| position - last);
        queue.last_cursor = Some(position);
        queue.cursor.push(CursorMoved {
            window: entity,
            position,
            delta,
        });
        glib::Propagation::Stop
    });
    let queue = input.clone();
    viewport.connect_scroll_event(move |_, event| {
        let (x, y) = match event.direction() {
            gdk::ScrollDirection::Up => (0.0, 1.0),
            gdk::ScrollDirection::Down => (0.0, -1.0),
            gdk::ScrollDirection::Left => (1.0, 0.0),
            gdk::ScrollDirection::Right => (-1.0, 0.0),
            _ => {
                let (x, y) = event.delta();
                (-x, -y)
            }
        };
        queue.borrow_mut().wheel.push(MouseWheel {
            window: entity,
            unit: MouseScrollUnit::Line,
            x: x as f32,
            y: y as f32,
        });
        glib::Propagation::Stop
    });
    let queue = input.clone();
    viewport.connect_enter_notify_event(move |_, _| {
        queue.borrow_mut().enter = true;
        glib::Propagation::Proceed
    });
    let queue = input.clone();
    viewport.connect_leave_notify_event(move |_, _| {
        let mut q = queue.borrow_mut();
        q.leave = true;
        q.last_cursor = None;
        glib::Propagation::Proceed
    });
    let queue = input.clone();
    viewport.connect_focus_out_event(move |_, _| {
        queue.borrow_mut().lost_focus = true;
        glib::Propagation::Proceed
    });
}

fn drain_input(world: &mut World, entity: Entity, input: &mut InputQueue) {
    for event in input.keys.drain(..) {
        world.write_message(event);
    }
    for event in input.buttons.drain(..) {
        world.write_message(event);
    }
    for event in input.cursor.drain(..) {
        if let Some(delta) = event.delta {
            world.write_message(MouseMotion { delta });
        }
        if let Some(mut window) = world.get_mut::<Window>(entity) {
            window.set_cursor_position(Some(event.position));
        }
        world.write_message(event);
    }
    for event in input.wheel.drain(..) {
        world.write_message(event);
    }
    if std::mem::take(&mut input.enter) {
        world.write_message(CursorEntered { window: entity });
    }
    if std::mem::take(&mut input.leave) {
        world.write_message(CursorLeft { window: entity });
    }
    if std::mem::take(&mut input.lost_focus) {
        world.write_message(KeyboardFocusLost);
        for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
            world.write_message(MouseButtonInput {
                window: entity,
                button,
                state: ButtonState::Released,
            });
        }
    }
}

// XKB keycodes used by GDK are Linux evdev codes + 8, independent of layout.
fn physical_key(xkb: u16) -> Option<KeyCode> {
    Some(match xkb.checked_sub(8)? {
        1 => KeyCode::Escape,
        16 => KeyCode::KeyQ,
        17 => KeyCode::KeyW,
        18 => KeyCode::KeyE,
        19 => KeyCode::KeyR,
        28 => KeyCode::Enter,
        30 => KeyCode::KeyA,
        31 => KeyCode::KeyS,
        32 => KeyCode::KeyD,
        33 => KeyCode::KeyF,
        42 => KeyCode::ShiftLeft,
        46 => KeyCode::KeyC,
        54 => KeyCode::ShiftRight,
        57 => KeyCode::Space,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serves_only_known_ui_assets() {
        assert!(asset("/ui/panels.js").is_some());
        for path in [
            "/.env",
            "/../Cargo.toml",
            "/data/space.sqlite3",
            "/ui/../../.env",
            "/galaxy",
        ] {
            assert!(asset(path).is_none());
        }
    }
    #[test]
    fn movement_keys_are_physical_and_unknown_keys_are_ignored() {
        assert_eq!(physical_key(25), Some(KeyCode::KeyW));
        assert_eq!(physical_key(50), Some(KeyCode::ShiftLeft));
        assert_eq!(physical_key(0), None);
    }
}
