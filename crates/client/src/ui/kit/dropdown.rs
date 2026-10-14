//! Button-styled dropdowns with shared easing for width, arrow and menu motion.
use super::interact::{ANIMATION_SECONDS, control_sense, ease_out, held_click, input_edges};
use super::{SCROLL_SOURCE, SPACING, Theme, animated_rect, list_row};
use bevy_egui::egui::{self, Align2, Rect, pos2, vec2};

/// Returns the selected stable option ID. A missing selection represents an
/// external query containing a combination rather than one menu option.
pub fn dropdown(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: &str,
    label: &str,
    selected: Option<&str>,
    options: &[(&str, &str)],
) -> Option<String> {
    ui.push_id(id, |ui| {
        let id = ui.make_persistent_id("dropdown");
        let menu_id = id.with("menu");
        let state_id = id.with("open");
        let mut open = ui.data_mut(|data| data.get_temp::<bool>(state_id).unwrap_or(false));
        let galley = ui
            .painter()
            .layout_no_wrap(label.into(), theme.font.id(), theme.text);
        let arrow_width = 6.0;
        let target_size = vec2(
            galley.size().x + arrow_width + 3.0 * SPACING,
            galley.size().y.max(ui.spacing().interact_size.y) + theme.input_padding().sum().y,
        );
        let target = Rect::from_min_size(egui::Pos2::ZERO, target_size);
        let size = animated_rect(ui, id.with("width"), target, target).size();
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let position_id = id.with("position");
        let moved = ui.data_mut(|data| {
            let previous = data.get_temp::<egui::Pos2>(position_id);
            data.insert_temp(position_id, rect.min);
            previous.is_some_and(|position| position.distance(rect.min) > 0.5)
        });
        if moved {
            open = false;
        }
        let response = held_click(ui.interact(rect, id, control_sense(true)))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            open = !open;
        }
        let previous_menu = ui.ctx().memory(|memory| memory.area_rect(menu_id));
        let outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| {
                    !rect.contains(pos) && !previous_menu.is_some_and(|menu| menu.contains(pos))
                })
        });
        if outside || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        let amount = ui.ctx().animate_bool_with_time_and_easing(
            id.with("opening"),
            open,
            ANIMATION_SECONDS,
            ease_out,
        );
        ui.painter().rect_filled(rect, 0, theme.input_fill());
        let arrow_center = pos2(rect.right() - SPACING - arrow_width * 0.5, rect.center().y);
        let text_clip = Rect::from_min_max(
            rect.min,
            pos2(arrow_center.x - arrow_width * 0.5 - SPACING, rect.bottom()),
        );
        ui.painter()
            .with_clip_rect(ui.clip_rect().intersect(text_clip))
            .galley(
                pos2(
                    rect.left() + SPACING,
                    rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                theme.text,
            );
        arrow(ui, theme, arrow_center, amount);
        input_edges(ui, theme, rect, id, response.hovered());
        let mut picked = None;
        if amount > 0.0 {
            let width = options
                .iter()
                .map(|(_, label)| {
                    ui.painter()
                        .layout_no_wrap((*label).into(), theme.font.id(), theme.text)
                        .size()
                        .x
                        + 2.0 * SPACING
                })
                // Row allocation rounds UI coordinates. Match that geometry
                // so its right highlight cannot get clipped by a fractional
                // container edge while the menu moves/reveals.
                .fold(rect.width(), f32::max)
                .ceil();
            let full_height = (options.len() as f32 * theme.field_height()).min(300.0);
            egui::Area::new(menu_id)
                .order(egui::Order::Tooltip)
                .fade_in(false)
                .fixed_pos(rect.left_bottom())
                .show(ui.ctx(), |ui| {
                    theme.style_scrollbars(ui.style_mut());
                    let (visible, _) = ui.allocate_exact_size(
                        vec2(width, full_height * amount),
                        egui::Sense::hover(),
                    );
                    let clip = ui.clip_rect().intersect(visible);
                    ui.painter().rect_filled(visible, 0, theme.fill.to_opaque());
                    // Reveal downwards while the content settles into place.
                    // Keeping exiting menus alive lets the same curve close them.
                    let content = Rect::from_min_size(
                        visible.min - vec2(0.0, SPACING * (1.0 - amount)),
                        vec2(width, full_height),
                    );
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(content)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    child.set_clip_rect(clip);
                    child.spacing_mut().item_spacing = vec2(SPACING, 0.0);
                    if !open {
                        child.disable();
                    }
                    super::ScrollArea::vertical()
                        .id_salt("options")
                        .scroll_bar_visibility(
                            if options.len() as f32 * theme.field_height() > full_height {
                                egui::scroll_area::ScrollBarVisibility::AlwaysVisible
                            } else {
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden
                            },
                        )
                        .scroll_source(SCROLL_SOURCE)
                        .min_scrolled_height(0.0)
                        .max_height(full_height)
                        .show(&mut child, |ui| {
                            for &(option_id, label) in options {
                                ui.push_id(option_id, |ui| {
                                    let chosen = selected == Some(option_id);
                                    let (row, response, _) =
                                        list_row(ui, theme, theme.field_height(), chosen);
                                    ui.painter().text(
                                        pos2(row.left() + SPACING, row.center().y),
                                        Align2::LEFT_CENTER,
                                        label,
                                        theme.font.id(),
                                        if chosen { theme.accent } else { theme.text },
                                    );
                                    if response.clicked() {
                                        picked = Some(option_id.to_owned());
                                        open = false;
                                    }
                                });
                            }
                        });
                });
        }
        ui.data_mut(|data| data.insert_temp(state_id, open));
        picked
    })
    .inner
}

pub(super) fn arrow(ui: &egui::Ui, theme: &Theme, center: egui::Pos2, amount: f32) {
    let rotation = egui::emath::Rot2::from_angle(amount * std::f32::consts::PI);
    let points = [vec2(-3.0, -1.5), vec2(0.0, 1.5), vec2(3.0, -1.5)]
        .map(|offset| center + rotation * offset);
    ui.painter()
        .line_segment([points[0], points[1]], (1.0, theme.dim));
    ui.painter()
        .line_segment([points[1], points[2]], (1.0, theme.dim));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdown_animates_width_arrow_and_opaque_edge_to_edge_menu() {
        let ctx = egui::Context::default();
        let theme = Theme::GAME;
        let frame = |time, label: &str, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 500.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        dropdown(
                            ui,
                            &theme,
                            "test",
                            label,
                            Some("a"),
                            &[("a", "First"), ("b", "Second")],
                        );
                    });
                },
            )
        };
        let button = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect) if rect.fill == theme.input_fill() => {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .unwrap()
        };
        let arrows = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::LineSegment { points, stroke }
                        if stroke.width == 1.0 && stroke.color == theme.dim =>
                    {
                        Some(*points)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let menu = |output: &egui::FullOutput| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill == theme.fill.to_opaque() => {
                    Some(rect.rect)
                }
                _ => None,
            })
        };
        let initial = frame(0.0, "Type: Star", vec![]);
        let rect = button(&initial);
        let down = arrows(&initial);
        assert_eq!(down.len(), 2);
        let center = (down[0][0] + down[1][1].to_vec2()) * 0.5 + vec2(0.0, 1.5);
        assert!((center.x - (rect.right() - SPACING - 3.0)).abs() < 0.01);
        let pos = rect.center();
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            0.01,
            "Type: Star",
            vec![egui::Event::PointerMoved(pos), press(true)],
        );
        frame(0.02, "Type: Star", vec![press(false)]);
        let opening = frame(0.11, "Type: Star", vec![]);
        let partial = menu(&opening).unwrap();
        assert!(partial.height() > 0.0 && partial.height() < 2.0 * theme.field_height());
        let right_edge = opening
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(edge)
                    if edge.fill == theme.accent
                        && edge.rect.width() == 2.0
                        && (edge.rect.right() - partial.right()).abs() < 0.5 =>
                {
                    Some((edge.rect, shape.clip_rect))
                }
                _ => None,
            })
            .expect("selected row must retain its right edge during opening");
        assert!(
            right_edge.0.right() <= right_edge.1.right() + 0.01,
            "the animated menu must not clip the right highlight"
        );
        let opened = frame(
            0.3,
            "Type: Star",
            vec![egui::Event::PointerMoved(pos2(650.0, 450.0))],
        );
        let full = menu(&opened).unwrap();
        assert!((full.height() - 2.0 * theme.field_height()).abs() < 0.01);
        assert!(
            opened.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Rect(row) if row.fill == theme.accent.gamma_multiply(0.12)
                && (row.rect.top() - full.top()).abs() < 0.01
                && (row.rect.width() - full.width()).abs() < 0.5)),
            "first item must start at the top and span the container without padding: menu {full:?}, rows {:?}",
            opened
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(row)
                        if row.fill == theme.accent.gamma_multiply(0.12) =>
                        Some(row.rect),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
        let up = arrows(&opened);
        assert!(
            (up[0][0] + down[0][0].to_vec2() - center.to_vec2() * 2.0)
                .to_vec2()
                .length()
                < 0.01
        );
        let idle_open = frame(0.5, "Type: Star", vec![]);
        assert!(menu(&idle_open).is_some());
        assert!(!idle_open.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Rect(line) if line.fill == theme.accent && line.rect.width() == 2.0 && (line.rect.top() - rect.top()).abs() < 0.01)),
            "an open menu must not retain the button edges without hover");
        frame(
            0.51,
            "Type: Star",
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let closing = frame(0.6, "Type: Star", vec![]);
        assert!(menu(&closing).unwrap().height() < full.height());
        assert!(menu(&frame(0.8, "Type: Star", vec![])).is_none());
        let changed = frame(0.81, "Type: Star system", vec![]);
        assert!((button(&changed).width() - rect.width()).abs() < 0.01);
        let growing = button(&frame(0.9, "Type: Star system", vec![])).width();
        let grown = button(&frame(1.1, "Type: Star system", vec![])).width();
        assert!(growing > rect.width() && grown > growing);
    }

    #[test]
    fn moving_the_parent_closes_the_open_menu() {
        let ctx = egui::Context::default();
        let theme = Theme::GAME;
        let frame = |time, offset, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 500.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add_space(offset);
                        dropdown(
                            ui,
                            &theme,
                            "test",
                            "Type: Star",
                            Some("a"),
                            &[("a", "Star")],
                        );
                    });
                },
            )
        };
        let output = frame(0.0, 0.0, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill == theme.input_fill() => {
                    Some(rect.rect.center())
                }
                _ => None,
            })
            .unwrap();
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(0.01, 0.0, vec![egui::Event::PointerMoved(pos), press(true)]);
        frame(0.02, 0.0, vec![press(false)]);
        let has_menu = |output: egui::FullOutput| {
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
            egui::epaint::Shape::Rect(rect) if rect.fill == theme.fill.to_opaque())
            })
        };
        assert!(has_menu(frame(0.3, 0.0, vec![])));
        frame(0.31, 30.0, vec![]);
        assert!(
            !has_menu(frame(0.6, 30.0, vec![])),
            "a moving parent must close its menu"
        );
    }
}
