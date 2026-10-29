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

    /// A point on the header handle of the open, floating or front-docked
    /// window `key`: its top padding, clear of the title and close button.
    fn header(&self, key: &str) -> Pos2 {
        self.settle();
        let bounds = if is_docked(&self.ctx, window_id(key)) {
            docked_rect(self, key)
        } else {
            self.floating(key)
        };
        Pos2::new(bounds.center().x, bounds.top() + 6.0)
    }

    /// Presses at `from`, moves through `path` a frame at a time, and
    /// releases at its end.
    fn carry(&self, from: Pos2, path: &[Pos2]) {
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![press(from, true)]);
        for point in path {
            self.frame(vec![Event::PointerMoved(*point)]);
        }
        self.frame(vec![press(*path.last().unwrap(), false)]);
        self.settle();
    }

    fn double_click(&self, pos: Pos2) {
        // Long enough after any earlier click not to count as a third.
        self.time.set(self.time.get() + 1.0);
        self.frame(vec![Event::PointerMoved(pos)]);
        for _ in 0..2 {
            self.frame(vec![press(pos, true)]);
            self.frame(vec![press(pos, false)]);
        }
        self.frame(vec![Event::PointerMoved(Pos2::new(5.0, 890.0))]);
        self.settle();
    }

    /// Carries the floating window `key` by its header to the screen's edge.
    fn dock(&self, key: &str) {
        let edge = Pos2::new(SCREEN.right() - 10.0, 450.0);
        self.carry(self.header(key), &[edge - egui::vec2(200.0, 0.0), edge]);
    }

    /// Where the tab titled `title` is drawn.
    fn tab(&self, title: &str) -> Pos2 {
        let output = self.settle();
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
    harness.dock("first");
    harness.settle();
    let scene = harness.scene();
    let docked = docked_rect(&harness, "first");
    assert!(
        (docked.width() - floating.width()).abs() < 1.0,
        "an empty sidebar takes the floating width: {floating:?} -> {docked:?}"
    );
    assert!((docked.right() - SCREEN.right()).abs() < 1.0);
    assert!(
        docked.top() > 20.0 && (docked.bottom() - SCREEN.bottom()).abs() < 1.0,
        "the window fills the sidebar below its tab: {docked:?}"
    );
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
    harness.dock("first");
    harness.double_click(harness.tab("first"));
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
    harness.dock("first");
    harness.settle();
    assert!(floating.is_some());
    assert_eq!(harness.content.get()[0], floating);
}

#[test]
fn several_docked_windows_share_the_sidebar_as_tabs() {
    let harness = Harness::new([true, true]);
    harness.settle();
    harness.dock("first");
    harness.dock("second");
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
    let first = harness.tab("first");
    harness.tab("second");
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
    harness.dock("first");
    harness.dock("second");
    harness.double_click(harness.tab("second"));
    let texts = Harness::texts(&harness.settle());
    assert!(texts.contains(&"first content".to_owned()));
    let docked = docked_rect(&harness, "first");
    assert!(
        docked.top() > 20.0,
        "even one tab shows in the strip, to drag"
    );
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
    harness.dock("first");
    harness.dock("second");
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
    harness.dock("first");
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
    harness.dock("first");
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

#[test]
fn later_drags_and_redocking_continue_from_the_chosen_whole_point_width() {
    let harness = Harness::new([true, false]);
    harness.settle();
    harness.dock("first");
    harness.settle();
    let drag = |from: f32, by: f32| {
        let edge = Pos2::new(from, 450.0);
        harness.frame(vec![Event::PointerMoved(edge)]);
        harness.frame(vec![press(edge, true)]);
        harness.frame(vec![Event::PointerMoved(edge + egui::vec2(by, 0.0))]);
        harness.frame(vec![press(edge + egui::vec2(by, 0.0), false)]);
        harness.settle();
        harness.scene().right()
    };
    let wide = drag(harness.scene().right(), -60.3);
    assert_eq!(wide, wide.round(), "the sidebar edge stays on whole points");
    // Grabbed just inside the docked window, the edge still resizes.
    let wider = drag(wide + 2.0, -50.0);
    assert!(
        (wider - (wide - 50.0)).abs() < 1.0,
        "a second drag continues from the chosen width: {wide} -> {wider}"
    );
    harness.double_click(harness.tab("first"));
    harness.settle();
    assert_eq!(harness.scene(), SCREEN);
    harness.dock("first");
    harness.settle();
    assert_eq!(
        harness.scene().right(),
        wider,
        "re-docking keeps the chosen width"
    );
}

/// Drop previews: accent outlines two points thick.
fn previews(output: &FullOutput) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Rect(rect) if rect.stroke.width == 2.0 => Some(rect.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn carrying_a_header_to_the_right_edge_previews_the_sidebar_then_docks() {
    let harness = Harness::new([true, false]);
    let from = harness.header("first");
    let floating = harness.floating("first");
    let edge = Pos2::new(SCREEN.right() - 10.0, 450.0);
    harness.frame(vec![Event::PointerMoved(from)]);
    harness.frame(vec![press(from, true)]);
    harness.frame(vec![Event::PointerMoved(Pos2::new(800.0, 450.0))]);
    assert!(carrying(&harness.ctx), "the pointer belongs to the carry");
    assert!(previews(&harness.frame(vec![])).is_empty());
    harness.frame(vec![Event::PointerMoved(edge)]);
    let preview = (0..5)
        .map(|_| harness.frame(vec![]))
        .last()
        .map(|output| previews(&output))
        .unwrap();
    assert_eq!(preview.len(), 1, "one preview while over the edge");
    assert!(
        (preview[0].width() - floating.width()).abs() < 4.0
            && (preview[0].right() - SCREEN.right()).abs() < 2.0,
        "the preview shows where the sidebar would open: {:?}",
        preview[0]
    );
    assert!(
        !is_docked(&harness.ctx, window_id("first")),
        "nothing docks before release"
    );
    harness.frame(vec![press(edge, false)]);
    harness.settle();
    assert!(is_docked(&harness.ctx, window_id("first")));
    assert!((harness.scene().right() - preview[0].left()).abs() < 4.0);
    assert!(!carrying(&harness.ctx));
}

#[test]
fn a_preview_fades_when_the_pointer_leaves_the_edge() {
    let harness = Harness::new([true, false]);
    let from = harness.header("first");
    harness.frame(vec![Event::PointerMoved(from)]);
    harness.frame(vec![press(from, true)]);
    harness.frame(vec![Event::PointerMoved(Pos2::new(
        SCREEN.right() - 5.0,
        450.0,
    ))]);
    harness.settle();
    let fading = previews(&harness.frame(vec![Event::PointerMoved(Pos2::new(700.0, 450.0))]));
    assert_eq!(fading.len(), 1, "it eases out rather than vanishing");
    harness.settle();
    harness.settle();
    assert!(previews(&harness.frame(vec![])).is_empty());
    harness.frame(vec![press(Pos2::new(700.0, 450.0), false)]);
    harness.settle();
    assert!(!is_docked(&harness.ctx, window_id("first")));
}

#[test]
fn carrying_a_header_elsewhere_only_moves_the_window_with_its_grab_point() {
    let harness = Harness::new([true, false]);
    let from = harness.header("first");
    let before = harness.floating("first");
    let to = Pos2::new(700.0, 300.0);
    harness.carry(from, &[Pos2::new(900.0, 200.0), to]);
    let after = harness.floating("first");
    assert!(
        ((after.min - before.min) - (to - from)).length() < 1.0,
        "{before:?} -> {after:?}"
    );
    assert_eq!(harness.scene(), SCREEN);
    assert!(!is_docked(&harness.ctx, window_id("first")));
}

#[test]
fn pulling_a_tab_out_floats_its_window_under_the_pointer() {
    let harness = Harness::new([true, false]);
    harness.dock("first");
    let tab = harness.tab("first");
    let sidebar = harness.scene().right();
    harness.frame(vec![Event::PointerMoved(tab)]);
    harness.frame(vec![press(tab, true)]);
    // Within the sidebar it stays docked.
    harness.frame(vec![Event::PointerMoved(tab + egui::vec2(0.0, 200.0))]);
    harness.frame(vec![]);
    assert!(is_docked(&harness.ctx, window_id("first")));
    let out = Pos2::new(sidebar - 200.0, 300.0);
    harness.frame(vec![Event::PointerMoved(out)]);
    harness.frame(vec![]);
    harness.frame(vec![]);
    assert!(!is_docked(&harness.ctx, window_id("first")));
    assert_eq!(
        harness.scene(),
        SCREEN,
        "the last tab out closes the sidebar"
    );
    assert!(
        carrying(&harness.ctx),
        "the carry continues after the tab is gone"
    );
    let follow = Pos2::new(500.0, 200.0);
    harness.frame(vec![Event::PointerMoved(follow)]);
    harness.frame(vec![]);
    let window = harness.floating("first");
    assert!(
        window.x_range().contains(follow.x)
            && (window.top()..window.top() + 40.0).contains(&follow.y),
        "the window hangs from its header under the pointer: {window:?}"
    );
    harness.frame(vec![press(follow, false)]);
    harness.settle();
    assert!(!carrying(&harness.ctx));
    assert_eq!(
        harness.floating("first"),
        window,
        "it stays where it was let go"
    );
}

#[test]
fn dragging_a_tab_along_the_strip_reorders_it() {
    let harness = Harness::new([true, true]);
    harness.dock("first");
    harness.dock("second");
    let (first, second) = (harness.tab("first"), harness.tab("second"));
    assert!(first.x < second.x);
    harness.carry(
        second,
        &[
            second - egui::vec2(10.0, 0.0),
            first - egui::vec2(20.0, 0.0),
        ],
    );
    let (first, second) = (harness.tab("first"), harness.tab("second"));
    assert!(second.x < first.x, "second now leads: {second:?} {first:?}");
    assert!(is_docked(&harness.ctx, window_id("second")));
}

#[test]
fn a_window_dropped_on_the_tab_strip_joins_where_it_lands() {
    let harness = Harness::new([true, true]);
    harness.dock("first");
    let first = harness.tab("first");
    harness.carry(
        harness.header("second"),
        &[Pos2::new(1000.0, 300.0), first - egui::vec2(20.0, 0.0)],
    );
    assert!(is_docked(&harness.ctx, window_id("second")));
    let texts = Harness::texts(&harness.settle());
    assert!(
        texts.contains(&"second content".to_owned()),
        "it comes to the front"
    );
    assert!(harness.tab("second").x < harness.tab("first").x);
}

#[test]
fn double_clicking_a_header_or_tab_switches_between_floating_and_docked() {
    let harness = Harness::new([true, false]);
    harness.double_click(harness.header("first"));
    assert!(is_docked(&harness.ctx, window_id("first")));
    harness.double_click(harness.header("first"));
    assert!(!is_docked(&harness.ctx, window_id("first")));
    harness.dock("first");
    harness.double_click(harness.tab("first"));
    assert!(!is_docked(&harness.ctx, window_id("first")));
    assert_eq!(harness.scene(), SCREEN);
}
