use super::*;

type PaintedChips = (Vec<(u8, Pill)>, Vec<(Rect, Rect)>);

fn frame(
    ctx: &egui::Context,
    chips: &mut Pills<u8>,
    time: f64,
    values: &[(u8, String)],
    events: Vec<egui::Event>,
) -> PaintedChips {
    let mut actions = Vec::new();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 500.0))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    actions = chips.show(ui, &Theme::GAME, values);
                });
            });
        },
    );
    let rects = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == Theme::GAME.input_fill() => {
                Some((rect.rect, shape.clip_rect))
            }
            _ => None,
        })
        .collect();
    (actions, rects)
}

#[test]
fn collection_retains_removed_chips_and_disables_their_actions_until_exit_finishes() {
    let ctx = egui::Context::default();
    let mut chips = Pills::default();
    let values = [(0, "Old filter".into()), (1, "Another filter".into())];
    frame(&ctx, &mut chips, 0.0, &values, vec![]);
    let full = frame(&ctx, &mut chips, 0.2, &values, vec![]).1;
    assert_eq!(full.len(), 2);
    frame(&ctx, &mut chips, 0.21, &[], vec![]);
    let exiting = frame(&ctx, &mut chips, 0.25, &[], vec![]).1;
    assert_eq!(exiting.len(), 2);
    for (rect, clip) in &exiting {
        assert!(clip.width() > 0.0 && clip.width() < rect.width());
    }
    let pos = exiting[0].1.center();
    let event = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    assert!(
        frame(
            &ctx,
            &mut chips,
            0.26,
            &[],
            vec![egui::Event::PointerMoved(pos), event(true)]
        )
        .0
        .is_empty()
    );
    assert!(
        frame(&ctx, &mut chips, 0.27, &[], vec![event(false)])
            .0
            .is_empty()
    );
    assert!(frame(&ctx, &mut chips, 0.41, &[], vec![]).1.is_empty());
    assert!(chips.entries.is_empty());
}

#[test]
fn reappearing_mid_exit_retargets_without_snapping_or_duplicate_chips() {
    let ctx = egui::Context::default();
    let mut chips = Pills::default();
    let values = [(0, "Filter".into())];
    frame(&ctx, &mut chips, 0.0, &values, vec![]);
    let full = frame(&ctx, &mut chips, 0.2, &values, vec![]).1[0].1.width();
    frame(&ctx, &mut chips, 0.21, &[], vec![]);
    let exiting = frame(&ctx, &mut chips, 0.3, &[], vec![]).1[0].1.width();
    let reopened = frame(&ctx, &mut chips, 0.3, &values, vec![]).1[0].1.width();
    assert_eq!(reopened, exiting);
    assert_eq!(chips.entries.len(), 1);
    let growing = frame(&ctx, &mut chips, 0.39, &values, vec![]).1[0]
        .1
        .width();
    assert!(growing > reopened && growing < full);
    assert_eq!(
        frame(&ctx, &mut chips, 0.5, &values, vec![]).1[0].1.width(),
        full
    );
    frame(&ctx, &mut chips, 0.51, &[], vec![]);
    assert!(frame(&ctx, &mut chips, 0.71, &[], vec![]).1.is_empty());
    assert!(frame(&ctx, &mut chips, 0.72, &values, vec![]).1.is_empty());
    let entering = frame(&ctx, &mut chips, 0.81, &values, vec![]).1[0]
        .1
        .width();
    assert!(
        entering > 0.0 && entering < full,
        "re-added chips must replay their entrance"
    );
}

#[test]
fn replacing_a_label_animates_both_old_and_new_visuals() {
    let ctx = egui::Context::default();
    let mut chips = Pills::default();
    let old = [(0, "Old value".into())];
    let new = [(0, "New value".into())];
    frame(&ctx, &mut chips, 0.0, &old, vec![]);
    frame(&ctx, &mut chips, 0.2, &old, vec![]);
    frame(&ctx, &mut chips, 0.21, &new, vec![]);
    let transitioning = frame(&ctx, &mut chips, 0.3, &new, vec![]).1;
    assert_eq!(transitioning.len(), 2);
    for (rect, clip) in transitioning {
        assert!(clip.width() > 0.0 && clip.width() < rect.width());
    }
    assert_eq!(frame(&ctx, &mut chips, 0.41, &new, vec![]).1.len(), 1);
    assert_eq!(chips.entries.len(), 1);
}
