//! Selection-following object window. Planet cross-sections preserve radial scale.
use super::{Cache, atmosphere::pressure_display, composition::material_label};
use crate::format::number;
use crate::navigation::objects::ObjectRef;
use crate::{
    Navigation, craft::Now, planet_loading::PlanetLoading, stars::stream::Starfield, ui::kit,
};
use bevy_egui::egui::{self, Color32};
use space_shared::{Galaxy, Layer, LayeredPlanet};

const THEME: kit::Theme = kit::Theme::GAME;
pub const WINDOW: &str = "object-inspector";

#[derive(Default)]
pub struct Inspector {
    pub open: bool,
    cache: Cache,
    target: Option<ObjectRef>,
    record: Option<LayeredPlanet>,
    selected: Option<usize>,
}

impl Inspector {
    pub fn invalidate(&mut self) {
        self.cache.invalidate();
    }

    fn reconcile(&mut self, target: Option<ObjectRef>, record: Option<LayeredPlanet>) {
        if self.target != target || self.record != record {
            self.selected = record
                .as_ref()
                .and_then(|r| (!r.layers.is_empty()).then_some(0));
            self.target = target;
            self.record = record;
        }
    }
}

pub struct View<'a> {
    pub nav: &'a Navigation,
    pub galaxy: &'a Galaxy,
    pub field: &'a Starfield,
    pub now: Now<'a>,
    pub loading: &'a PlanetLoading,
}

pub fn show(ctx: &egui::Context, state: &mut Inspector, view: View<'_>) {
    if !state.open {
        return;
    }
    let target = view.nav.selection.as_ref().map(|s| s.object());
    let info = target.as_ref().map(|object| {
        state.cache.get(
            object,
            view.galaxy,
            super::position_ly(object, view.nav, view.galaxy, view.field),
            view.now,
            Some(view.loading),
            view.nav.system(view.galaxy),
        )
    });
    let title = info.as_ref().map(|i| i.title.clone()).unwrap_or_default();
    let interior = info.as_ref().and_then(|i| i.interior.clone());
    let rows: Vec<_> = info
        .as_ref()
        .map(|i| {
            i.rows()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        })
        .unwrap_or_default();
    state.reconcile(target, interior);
    let mut open = state.open;
    kit::Window::new(WINDOW, "Object Inspector")
        .size(kit::Extent::new(0.58, 620.0, 1000.0), kit::Extent::new(0.75, 480.0, 900.0))
        .min_size(egui::vec2(380.0, 320.0))
        .show(ctx, &THEME, &mut open, |ui| {
        kit::ScrollArea::vertical().id_salt("object-inspector-content").show(ui, |ui| {
            if state.target.is_none() {
                kit::note(ui, &THEME, "Select an object to inspect it.");
                return;
            }
            kit::heading(ui, &THEME, if title.is_empty() { "Object unavailable" } else { &title });
            if let Some(record) = &state.record {
                kit::note(ui, &THEME, "Active model · radial cross-section to scale · colours identify regions, not actual appearance");
                ui.add_space(kit::SPACING);
                if extent(&record.layers).is_some() {
                    if ui.available_width() >= 680.0 {
                        ui.columns(2, |columns| {
                            diagram(&mut columns[0], &record.layers, &mut state.selected);
                            overview(&mut columns[1], record, &mut state.selected);
                        });
                    } else {
                        diagram(ui, &record.layers, &mut state.selected);
                        overview(ui, record, &mut state.selected);
                    }
                } else {
                    kit::note(ui, &THEME, "No drawable layer geometry is available.");
                    overview(ui, record, &mut state.selected);
                }
            } else {
                if matches!(state.target, Some(ObjectRef::Planet { .. })) {
                    kit::note(ui, &THEME, "No validated active layer model is available.");
                }
                let facts: Vec<_> = rows.iter().map(|(k,v)| (k.as_str(),v.clone())).collect();
                kit::facts(ui, &THEME, "object-inspector-facts", &facts);
            }
        });
    });
    state.open = open;
}

fn extent(layers: &[Layer]) -> Option<f64> {
    if layers.is_empty()
        || layers.iter().any(|l| {
            !l.inner_radius_m.is_finite()
                || !l.outer_radius_m.is_finite()
                || l.inner_radius_m < 0.0
                || l.outer_radius_m < l.inner_radius_m
        })
    {
        return None;
    }
    let radius = layers.iter().map(|l| l.outer_radius_m).fold(0.0, f64::max);
    (radius > 0.0).then_some(radius)
}

fn hit(layers: &[Layer], radius: f64) -> Option<usize> {
    layers
        .iter()
        .enumerate()
        .rev()
        .find(|(_, l)| radius >= l.inner_radius_m && radius <= l.outer_radius_m)
        .map(|(i, _)| i)
}

fn colour(kind: &str) -> Color32 {
    let rgb = match kind {
        "atmosphere" | "envelope" => [101, 140, 170],
        "ocean" => [55, 126, 170],
        "ice" | "deep_ice" | "frost" => [142, 188, 201],
        "core" => [217, 163, 78],
        "mantle" => [181, 120, 73],
        _ => [153, 133, 116],
    };
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn label(layer: &Layer) -> String {
    layer.kind.replace('_', " ")
}

fn diagram(ui: &mut egui::Ui, layers: &[Layer], selected: &mut Option<usize>) {
    kit::eyebrow(ui, &THEME, "Interior cross-section");
    let size = ui.available_width().min(380.0);
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
    let Some(extent) = extent(layers) else {
        return;
    };
    let radius = (size * 0.5 - 8.0).max(1.0);
    let centre = rect.center();
    // Annular meshes, not overpainted discs: gaps and very thin regions retain
    // their real geometry. No minimum thickness or selection stroke widens them.
    for (index, layer) in layers.iter().enumerate() {
        let inner = (layer.inner_radius_m / extent) as f32 * radius;
        let outer = (layer.outer_radius_m / extent) as f32 * radius;
        let colour = if *selected == Some(index) {
            colour(&layer.kind).lerp_to_gamma(THEME.accent, 0.35)
        } else {
            colour(&layer.kind)
        };
        let mut mesh = egui::Mesh::default();
        for step in 0..=192 {
            let angle = step as f32 / 192.0 * std::f32::consts::TAU;
            let dir = egui::vec2(angle.cos(), angle.sin());
            mesh.colored_vertex(centre + dir * inner, colour);
            mesh.colored_vertex(centre + dir * outer, colour);
            if step > 0 {
                let n = step * 2;
                mesh.add_triangle(n - 2, n - 1, n);
                mesh.add_triangle(n - 1, n + 1, n);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
    }
    let pointed = response
        .hover_pos()
        .and_then(|p| hit(layers, f64::from(p.distance(centre) / radius) * extent));
    if let Some(index) = pointed {
        if response.clicked() {
            *selected = Some(index);
        }
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(label(&layers[index]));
    }
    kit::note(
        ui,
        &THEME,
        &format!(
            "Outer radius {} km · true radial scale",
            number(extent / 1000.0, 3)
        ),
    );
    kit::note(
        ui,
        &THEME,
        "Thin layers may be invisible at this scale. Select them in the layer overview.",
    );
}

fn overview(ui: &mut egui::Ui, record: &LayeredPlanet, selected: &mut Option<usize>) {
    kit::eyebrow(ui, &THEME, "Layers · exterior to centre");
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, layer) in record.layers.iter().enumerate() {
            let (rect, response, _) = kit::list_row(ui, &THEME, 46.0, *selected == Some(index));
            let left = rect.left() + THEME.inset;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(left, rect.center().y - 5.0),
                    egui::vec2(10.0, 10.0),
                ),
                0.0,
                colour(&layer.kind),
            );
            ui.painter().text(
                egui::pos2(left + 20.0, rect.top() + 5.0),
                egui::Align2::LEFT_TOP,
                label(layer),
                THEME.font.id(),
                THEME.text,
            );
            ui.painter().text(
                egui::pos2(left + 20.0, rect.top() + 26.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{} · {} km thick",
                    material_label(&layer.material),
                    number((layer.outer_radius_m - layer.inner_radius_m) / 1000.0, 3)
                ),
                THEME.small.id(),
                THEME.dim,
            );
            if response.clicked() {
                *selected = Some(index);
            }
        }
    });
    if let Some(layer) = selected.and_then(|i| record.layers.get(i)) {
        ui.add_space(kit::SPACING * 2.0);
        kit::eyebrow(ui, &THEME, "Selected layer");
        kit::heading(ui, &THEME, &label(layer));
        kit::facts(
            ui,
            &THEME,
            "layer-details",
            &[
                ("Material", material_label(&layer.material).to_owned()),
                (
                    "Inner radius",
                    format!("{} km", number(layer.inner_radius_m / 1000.0, 3)),
                ),
                (
                    "Outer radius",
                    format!("{} km", number(layer.outer_radius_m / 1000.0, 3)),
                ),
                (
                    "Thickness",
                    format!(
                        "{} km",
                        number((layer.outer_radius_m - layer.inner_radius_m) / 1000.0, 3)
                    ),
                ),
                ("Mass", format!("{:.4e} kg", layer.mass_kg)),
                (
                    "Inner pressure",
                    pressure_display(layer.inner_pressure_pa).1,
                ),
                (
                    "Outer pressure",
                    layer
                        .outer_pressure_pa
                        .map(|p| pressure_display(p).1)
                        .unwrap_or_else(|| "Unknown · top pressure unresolved".into()),
                ),
                (
                    "Temperature",
                    layer
                        .temperature_k
                        .map(|t| format!("{} K", number(t, 0)))
                        .unwrap_or_else(|| "Unknown · model does not resolve temperature".into()),
                ),
                ("Model", layer.model.clone()),
                (
                    "Fidelity",
                    super::layered::fidelity_label(layer.model_fidelity).into(),
                ),
            ],
        );
        ui.add_space(kit::SPACING);
        kit::eyebrow(ui, &THEME, "Composition · % of layer mass");
        let mut materials: Vec<_> = layer.materials.iter().collect();
        materials.sort_by(|a, b| b.mass_kg.total_cmp(&a.mass_kg));
        let rows: Vec<_> = materials
            .iter()
            .map(|m| {
                (
                    material_label(&m.material),
                    format!(
                        "{} · {:.4e} kg",
                        super::composition::percent(m.mass_kg / layer.mass_kg),
                        m.mass_kg
                    ),
                )
            })
            .collect();
        if rows.is_empty() {
            kit::note(ui, &THEME, "Composition unavailable");
        } else {
            kit::facts(ui, &THEME, "layer-composition", &rows);
        }
    }
}

#[cfg(test)]
mod tests;
