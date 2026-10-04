//! Actual GTK widget signal -> Bevy input -> camera smoke test. Synthetic
//! events do not move the user's mouse or take focus from another application.
use super::*;
use bevy::math::DVec3;
use glib::translate::ToGlibPtrMut;

pub(super) struct Smoke {
    stage: u8,
    start: Instant,
    deadline: Instant,
    size: (i32, i32),
    eye: DVec3,
    range: f64,
    rotation: Quat,
    checks: Vec<&'static str>,
}

impl Smoke {
    pub fn new() -> Self {
        Self {
            stage: 0,
            start: Instant::now(),
            deadline: Instant::now() + Duration::from_secs(30),
            size: (0, 0),
            eye: DVec3::ZERO,
            range: 0.0,
            rotation: Quat::IDENTITY,
            checks: Vec::new(),
        }
    }
    pub fn step(
        &mut self,
        app: &mut App,
        window: &gtk::Window,
        area: &gtk::DrawingArea,
        webview: &WebView,
        ui: Option<bool>,
    ) {
        if self.stage == 255 {
            return;
        }
        if Instant::now() > self.deadline || ui == Some(false) {
            self.finish(
                window,
                Some("GTK host smoke timed out or shared UI test failed"),
            );
            return;
        }
        let size = (area.allocated_width(), area.allocated_height());
        match self.stage {
            0 if ui == Some(true) => {
                self.checks.push("shared HTML controls");
                self.size = size;
                // Wayland's compositor owns toplevel sizing and can ignore a
                // client resize request. Force a real GTK layout allocation
                // change instead, exercising the same swapchain-resize path.
                webview.set_size_request(500, -1);
                self.stage = 1;
            }
            1 if size != self.size => {
                let entity = app
                    .world_mut()
                    .query_filtered::<Entity, With<PrimaryWindow>>()
                    .single(app.world())
                    .unwrap();
                let resolution = &app.world().get::<Window>(entity).unwrap().resolution;
                if resolution.width() != size.0 as f32 || resolution.height() != size.1 as f32 {
                    self.finish(window, Some("GTK and Bevy viewport sizes disagree"));
                    return;
                }
                self.checks
                    .push("GTK layout resize and swapchain dimensions");
                app.world_mut().write_message(UiCommand::SetView {
                    view: crate::views::View::Orbit,
                });
                self.stage = 2;
            }
            2 if !app.world().resource::<crate::scene::CameraRig>().flying()
                && app.world().resource::<crate::Navigation>().view
                    == crate::views::View::Orbit =>
            {
                self.eye = app.world().resource::<crate::scene::CameraPose>().eye;
                key(area, true);
                self.start = Instant::now();
                self.stage = 3;
            }
            3 if self.start.elapsed() > Duration::from_millis(350) => {
                key(area, false);
                if app
                    .world()
                    .resource::<crate::scene::CameraPose>()
                    .eye
                    .distance(self.eye)
                    < 1.0
                {
                    self.finish(window, Some("GTK key event did not move the native camera"));
                    return;
                }
                self.checks.push("GTK keyboard signal moves native camera");
                self.range = app.world().resource::<crate::Navigation>().distance;
                scroll(area);
                self.start = Instant::now();
                self.stage = 4;
            }
            4 if self.start.elapsed() > Duration::from_millis(100) => {
                if app.world().resource::<crate::Navigation>().distance <= self.range {
                    self.finish(
                        window,
                        Some("GTK wheel event did not zoom the native camera"),
                    );
                    return;
                }
                self.checks.push("GTK wheel signal zooms native camera");
                self.rotation = app.world().resource::<crate::Navigation>().rotation;
                pointer(area, gdk::EventType::MotionNotify, 120.0, 140.0);
                pointer(area, gdk::EventType::ButtonPress, 120.0, 140.0);
                pointer(area, gdk::EventType::MotionNotify, 160.0, 170.0);
                self.start = Instant::now();
                self.stage = 5;
            }
            5 if self.start.elapsed() > Duration::from_millis(100) => {
                pointer(area, gdk::EventType::ButtonRelease, 160.0, 170.0);
                if app
                    .world()
                    .resource::<crate::Navigation>()
                    .rotation
                    .dot(self.rotation)
                    .abs()
                    > 0.999999
                {
                    self.finish(
                        window,
                        Some("GTK right-drag did not turn the native camera"),
                    );
                    return;
                }
                self.checks.push("GTK pointer signals orbit native camera");
                key(area, true);
                webview.grab_focus();
                // The test window never takes global keyboard focus. Simulate
                // the focus-out signal that GTK delivers on a real handoff.
                area.emit_by_name::<bool>(
                    "focus-out-event",
                    &[&gdk::Event::new(gdk::EventType::FocusChange)],
                );
                webview.evaluate_javascript("const field = document.querySelector('input[aria-label=\"Filter lists\"]'); field.focus(); field.value = 'wasd'; field.dispatchEvent(new KeyboardEvent('keydown', {key:'w', bubbles:true}));", None, None, gio::Cancellable::NONE, |_| {});
                self.start = Instant::now();
                self.stage = 6;
            }
            6 if self.start.elapsed() > Duration::from_millis(400) => {
                let keys = app.world().resource::<ButtonInput<KeyCode>>();
                let buttons = app.world().resource::<ButtonInput<MouseButton>>();
                if keys.pressed(KeyCode::KeyW) || buttons.pressed(MouseButton::Right) {
                    self.finish(
                        window,
                        Some("Viewport input remained held after HTML focus handoff"),
                    );
                    return;
                }
                self.checks
                    .push("HTML focus handoff clears held viewport input");
                self.finish(window, None);
            }
            _ => {}
        }
    }
    fn finish(&mut self, window: &gtk::Window, error: Option<&str>) {
        let report = serde_json::json!({"ok": error.is_none(), "checks": self.checks, "error": error, "events": "synthetic GTK widget signals, not global input injection"});
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/logs");
        let _ = std::fs::create_dir_all(&root);
        let _ = std::fs::write(root.join("gtk-host-smoke.json"), report.to_string());
        eprintln!("GTK host smoke: {report}");
        self.stage = 255;
        window.close(); // Exercises the normal renderer-before-surface shutdown.
    }
}

fn key(area: &gtk::DrawingArea, pressed: bool) {
    let mut event = gdk::Event::new(if pressed {
        gdk::EventType::KeyPress
    } else {
        gdk::EventType::KeyRelease
    });
    // SAFETY: Fresh, owned GDK event, matching union variant, scalar fields only.
    unsafe {
        let raw: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
        (*raw).key.hardware_keycode = 25;
        (*raw).key.keyval = u32::from('w');
    }
    area.emit_by_name::<bool>(
        if pressed {
            "key-press-event"
        } else {
            "key-release-event"
        },
        &[&event],
    );
}
fn scroll(area: &gtk::DrawingArea) {
    let mut event = gdk::Event::new(gdk::EventType::Scroll);
    // SAFETY: Fresh, owned scroll event, matching union variant.
    unsafe {
        let raw: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
        (*raw).scroll.direction = gdk::ffi::GDK_SCROLL_DOWN;
    }
    area.emit_by_name::<bool>("scroll-event", &[&event]);
}
fn pointer(area: &gtk::DrawingArea, kind: gdk::EventType, x: f64, y: f64) {
    let mut event = gdk::Event::new(kind);
    let signal = if kind == gdk::EventType::MotionNotify {
        "motion-notify-event"
    } else if kind == gdk::EventType::ButtonPress {
        "button-press-event"
    } else {
        "button-release-event"
    };
    // SAFETY: Fresh, owned event; each write matches the selected union variant.
    unsafe {
        let raw: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
        if kind == gdk::EventType::MotionNotify {
            (*raw).motion.x = x;
            (*raw).motion.y = y;
        } else {
            (*raw).button.button = 3;
            (*raw).button.x = x;
            (*raw).button.y = y;
        }
    }
    area.emit_by_name::<bool>(signal, &[&event]);
}
