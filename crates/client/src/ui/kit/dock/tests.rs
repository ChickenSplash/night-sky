use super::super::{Theme, Window, layout, reveal_window, toggle_window, window_id};
use super::*;
use bevy_egui::egui::{Event, FullOutput, PointerButton, Pos2, epaint::Shape};
use std::cell::Cell;

const SCREEN: Rect = Rect {
    min: Pos2::ZERO,
    max: Pos2::new(1600.0, 900.0),
};
const KEYS: [&str; 2] = ["first", "second"];

struct Harness {
    ctx: egui::Context,
    time: Cell<f64>,
    open: Cell<[bool; 2]>,
    /// Each window's content ID, as drawn in the last frame.
    content: Cell<[Option<Id>; 2]>,
}

impl Harness {
    fn new(open: [bool; 2]) -> Self {
        let ctx = egui::Context::default();
        super::super::plain_style(&ctx);
        // Both would open at the top right, one over the other.
        ctx.data_mut(|data| {
            data.insert_temp(Id::new(("window-center", KEYS[1])), Pos2::new(450.0, 450.0))
        });
        Self {
            ctx,
            time: Cell::new(0.0),
            open: Cell::new(open),
            content: Cell::new([None; 2]),
        }
    }

    fn frame(&self, events: Vec<Event>) -> FullOutput {
        self.time.set(self.time.get() + 0.05);
        let mut open = self.open.get();
        let mut content = [None; 2];
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(SCREEN),
                time: Some(self.time.get()),
                events,
                ..Default::default()
            },
            |ctx| {
                begin(ctx);
                for (index, key) in KEYS.into_iter().enumerate() {
                    Window::new(key, key).show(ctx, &Theme::GAME, &mut open[index], |ui| {
                        content[index] = Some(ui.id());
                        ui.label(format!("{key} content"));
                        ui.allocate_space(ui.available_size());
                    });
                }
            },
        );
        self.open.set(open);
        self.content.set(content);
        output
    }

    fn settle(&self) -> FullOutput {
        self.frame(vec![]);
        self.frame(vec![])
    }

    fn click(&self, pos: Pos2) {
        self.frame(vec![Event::PointerMoved(pos), press(pos, true)]);
        self.frame(vec![press(pos, false)]);
        self.frame(vec![Event::PointerMoved(Pos2::new(5.0, 890.0))]);
    }

    fn floating(&self, key: &str) -> Rect {
        self.ctx
            .memory(|memory| memory.area_rect(window_id(key)))
            .unwrap()
    }

    fn scene(&self) -> Rect {
        layout::scene(&self.ctx)
    }

    /// Where the open, floating or front-docked window `key` shows its button.
    fn dock_button(&self, key: &str) -> Pos2 {
        let output = self.settle();
        let area = if super::super::window::is_docked(&self.ctx, window_id(key)) {
            window_id(key).with("docked-area")
        } else {
            window_id(key)
        };
        let bounds = self.ctx.memory(|memory| memory.area_rect(area)).unwrap();
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::Rect(rect)
                    if rect.rect.size() == egui::vec2(16.0, 12.0)
                        && bounds.contains_rect(rect.rect) =>
                {
                    Some(rect.rect.center())
                }
                _ => None,
            })
            .expect("dock button is painted")
    }

    fn texts(output: &FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }
}

fn press(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn docked_rect(harness: &Harness, key: &str) -> Rect {
    harness
        .ctx
        .memory(|memory| memory.area_rect(window_id(key).with("docked-area")))
        .unwrap()
}

#[test]
fn docking_fills_the_right_column_and_shrinks_the_scene_beside_it() {
    let harness = Harness::new([true, false]);
    harness.settle();
    let floating = harness.floating("first");
    assert_eq!(harness.scene(), SCREEN);
    harness.click(harness.dock_button("first"));
    harness.settle();
    let scene = harness.scene();
    let docked = docked_rect(&harness, "first");
    assert!(
        (docked.width() - floating.width()).abs() < 1.0,
        "an empty sidebar takes the floating width: {floating:?} -> {docked:?}"
    );
    assert!((docked.right() - SCREEN.right()).abs() < 1.0);
    assert!(docked.top().abs() < 1.0 && (docked.bottom() - SCREEN.bottom()).abs() < 1.0);
    assert_eq!(scene.min, SCREEN.min);
    assert_eq!(scene.max.y, SCREEN.max.y);
    assert!(
        (scene.right() - docked.left()).abs() < 1.0,
        "nothing of the scene lies behind the sidebar: {scene:?}, {docked:?}"
    );
    assert!(harness.open.get()[0], "docking keeps the window open");
}

#[test]
fn floating_again_restores_the_previous_floating_rect_and_full_scene() {
    let harness = Harness::new([true, false]);
    harness.settle();
    let before = harness.floating("first");
    harness.click(harness.dock_button("first"));
    harness.click(harness.dock_button("first"));
    harness.settle();
    let after = harness.floating("first");
    assert!(
        (after.min - before.min).length() < 1.0 && (after.max - before.max).length() < 1.0,
        "{before:?} -> {after:?}"
    );
    assert_eq!(harness.scene(), SCREEN);
}

#[test]
fn content_keeps_one_id_whether_floating_or_docked() {
    let harness = Harness::new([true, false]);
    harness.settle();
    let floating = harness.content.get()[0];
    harness.click(harness.dock_button("first"));
    harness.settle();
    assert!(floating.is_some());
    assert_eq!(harness.content.get()[0], floating);
}

#[test]
fn several_docked_windows_share_the_sidebar_as_tabs() {
    let harness = Harness::new([true, true]);
    harness.settle();
    harness.click(harness.dock_button("first"));
    harness.click(harness.dock_button("second"));
    let output = harness.settle();
    let texts = Harness::texts(&output);
    assert!(
        texts.contains(&"second content".to_owned()),
        "newly docked tab is in front"
    );
    assert!(
        !texts.contains(&"first content".to_owned()),
        "only the front tab draws"
    );
    let tab = |output: &FullOutput, title: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::Text(text) if text.galley.text() == title => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .filter(|pos| pos.y < 80.0)
            .unwrap_or_else(|| panic!("{title} tab"))
    };
    let first = tab(&output, "first");
    tab(&output, "second");
    let docked = docked_rect(&harness, "second");
    assert!(
        docked.top() > 20.0,
        "the window sits below the tab strip: {docked:?}"
    );
    harness.click(first);
    let texts = Harness::texts(&harness.settle());
    assert!(texts.contains(&"first content".to_owned()));
    assert!(!texts.contains(&"second content".to_owned()));
}

#[test]
fn closing_or_floating_the_front_tab_shows_the_next_then_frees_the_scene() {
    let harness = Harness::new([true, true]);
    harness.settle();
    harness.click(harness.dock_button("first"));
    harness.click(harness.dock_button("second"));
    harness.click(harness.dock_button("second"));
    let texts = Harness::texts(&harness.settle());
    assert!(texts.contains(&"first content".to_owned()));
    let docked = docked_rect(&harness, "first");
    assert!(docked.top().abs() < 1.0, "one tab needs no strip");
    // A shortcut closes it without its button; it leaves the sidebar.
    harness.open.set([false, true]);
    harness.settle();
    assert_eq!(harness.scene(), SCREEN);
    // Reopening remembers it was docked.
    harness.open.set([true, true]);
    harness.settle();
    assert!(harness.scene().right() < SCREEN.right() - 100.0);
}

#[test]
fn shortcuts_bring_a_background_tab_forward_before_closing_it() {
    let harness = Harness::new([true, true]);
    harness.settle();
    harness.click(harness.dock_button("first"));
    harness.click(harness.dock_button("second"));
    harness.settle();
    let mut open = harness.open.get();
    toggle_window(&harness.ctx, "first", &mut open[0]);
    assert!(
        open[0],
        "a background tab comes forward rather than closing"
    );
    harness.open.set(open);
    let texts = Harness::texts(&harness.settle());
    assert!(texts.contains(&"first content".to_owned()));
    let mut open = harness.open.get();
    toggle_window(&harness.ctx, "first", &mut open[0]);
    assert!(!open[0], "the front tab closes");
    reveal_window(&harness.ctx, "first", &mut open[0]);
    assert!(open[0]);
}

#[test]
fn sidebar_edge_resizes_instantly_between_window_minimum_and_half_screen() {
    let harness = Harness::new([true, false]);
    harness.settle();
    harness.click(harness.dock_button("first"));
    harness.settle();
    let start = docked_rect(&harness, "first");
    let edge = Pos2::new(start.left(), 450.0);
    harness.frame(vec![Event::PointerMoved(edge)]);
    let hover = harness.frame(vec![]);
    assert_eq!(
        hover.platform_output.cursor_icon,
        egui::CursorIcon::ResizeHorizontal
    );
    harness.frame(vec![press(edge, true)]);
    harness.frame(vec![Event::PointerMoved(edge - egui::vec2(60.0, 0.0))]);
    harness.frame(vec![]);
    assert!(
        (harness.scene().right() - (start.left() - 60.0)).abs() < 1.0,
        "the drag applies at once, without easing"
    );
    harness.frame(vec![Event::PointerMoved(Pos2::new(0.0, 450.0))]);
    harness.frame(vec![]);
    assert!((harness.scene().right() - SCREEN.width() * MAX_SHARE).abs() < 1.0);
    harness.frame(vec![Event::PointerMoved(Pos2::new(SCREEN.right(), 450.0))]);
    harness.frame(vec![]);
    let theme = Theme::GAME;
    let minimum = 320.0 + 2.0 * (f32::from(theme.padding) + theme.border_width);
    assert!((SCREEN.right() - harness.scene().right() - minimum).abs() < 1.0);
    harness.frame(vec![press(Pos2::new(SCREEN.right(), 450.0), false)]);
}

#[test]
fn sidebar_edge_lights_from_the_pointer_like_window_edges() {
    let harness = Harness::new([true, false]);
    harness.settle();
    harness.click(harness.dock_button("first"));
    harness.settle();
    let left = harness.scene().right();
    let lines = |output: FullOutput| {
        output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                Shape::LineSegment { points, stroke } if stroke.color == Theme::GAME.accent => {
                    Some(points)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    harness.frame(vec![Event::PointerMoved(Pos2::new(left, 270.0))]);
    let midway = lines(harness.frame(vec![]));
    assert_eq!(midway.len(), 1);
    let [top, bottom] = midway[0];
    assert!(top.y > 0.0 && top.y < 270.0 && bottom.y > 270.0 && bottom.y < SCREEN.bottom());
    // 30% down the edge, so it grows three parts up for every seven down.
    assert!(((270.0 - top.y) / (bottom.y - 270.0) - 3.0 / 7.0).abs() < 0.02);
}
