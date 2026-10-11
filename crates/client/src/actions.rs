//! Every panel action goes through `UiCommand`, whether it comes from egui or a
//! browser test script (`automation.rs`). Lists and tasks are named by ID, so a
//! data refresh that reorders them cannot redirect a click.
use crate::{Clock, Navigation, Selection, Universe, presentation::PresentationMode};
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Message, Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    /// Select a streamed star by stable ID, or visit it on a second click.
    PickStar {
        id: String,
    },
    /// Open a list's constellation from any view.
    Jump {
        list: String,
    },
    /// Fly to a task's page in its list and select it.
    FindTask {
        list: String,
        task: String,
    },
    /// Select any planet by stable ID without moving the camera or visiting.
    Inspect {
        planet: String,
    },
    /// Activate an inspector tab without changing selection or camera state.
    InspectObject {
        inspection: crate::ui::breadcrumb::Inspection,
    },
    ShowPage {
        page: u32,
    },
    SetPresentationMode {
        presentation_mode: PresentationMode,
    },
    /// Fly to any star's system by stable ID and present it.
    VisitStar {
        id: String,
        presentation_mode: PresentationMode,
    },
    /// Fly to any planet by stable ID and follow it, visiting its star first.
    FlyToPlanet {
        planet: String,
    },
    Follow,
    /// Reframe a tracked planet's system, else clear selection or climb the trail.
    Up,
    SetPaused {
        paused: bool,
    },
    SetRate {
        rate: f64,
    },
    /// Camera travel speed in metres per second.
    SetMoveSpeed {
        speed: f64,
    },
    SetDisplay {
        option: Display,
        on: bool,
    },
    /// Vertical field of view, clamped to `FOV_DEG`.
    SetFov {
        degrees: f32,
    },
    /// Faintest star point magnitude, clamped to `MAGNITUDE`. Changing it
    /// reloads the stars.
    SetMagnitude {
        magnitude: f64,
    },
    SetExposure {
        stops: f64,
    },
    SetCompression {
        amount: f64,
    },
    /// Repeatable views for evaluating the smooth galaxy prototype.
    GalaxyReference {
        view: GalaxyReference,
    },
    /// Debug-only performance switches (`render/debug.rs`).
    SetDebug {
        switch: crate::render::debug::DebugSwitch,
        on: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    Exaggerate,
    Orbits,
    Labels,
    Markers,
    VelocityMarker,
    Realistic,
    GalaxyGlow,
    Wireframe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GalaxyReference {
    Inside,
    FaceOn,
    EdgeOn,
}

fn galaxy_reference(nav: &mut Navigation, view: GalaxyReference) {
    use bevy::math::DVec3;
    use space_simulation::galaxy::{GalaxyModel, PC_METRES};
    let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let eye = match view {
        GalaxyReference::Inside => home,
        GalaxyReference::FaceOn => DVec3::new(0.0, 38000.0, 0.0),
        GalaxyReference::EdgeOn => DVec3::new(0.0, 0.0, 38000.0),
    };
    let up = if view == GalaxyReference::FaceOn {
        Vec3::Z
    } else {
        Vec3::Y
    };
    nav.visit = None;
    nav.page = 0;
    nav.selection = None;
    nav.following = false;
    nav.presentation_mode = PresentationMode::Simulation;
    // Keep the normal zoom range even for an external reference eye. Otherwise
    // the first wheel event clamps 124,000 ly to 40,000 ly and jumps inward.
    nav.distance = (eye.length() * PC_METRES).min(40_000.0 * space_simulation::LY_METRES);
    nav.target = (eye - home) * PC_METRES - eye.normalize() * nav.distance;
    nav.rotation = Transform::default()
        .looking_to((-eye).as_vec3(), up)
        .rotation;
    nav.eye = Some((eye - home) * PC_METRES);
    nav.set_fov(50.0);
    nav.realistic = true;
    nav.fly = true;
    nav.notice = "Smooth galaxy prototype: no dust or spiral detail yet".into();
}

pub fn apply(
    mut commands: MessageReader<UiCommand>,
    universe: Res<Universe>,
    mut nav: ResMut<Navigation>,
    mut clock: ResMut<Clock>,
    field: Res<crate::stars::stream::Starfield>,
    mut debug: ResMut<crate::render::debug::RenderDebug>,
) {
    let galaxy = &universe.0;
    let list = |id: &str| galaxy.systems.iter().position(|s| s.id == id);
    let mut queue: std::collections::VecDeque<_> = commands.read().cloned().collect();
    while let Some(command) = queue.pop_front() {
        match &command {
            UiCommand::PickStar { id } => {
                if nav.selected_star().is_some_and(|star| star.id == *id) {
                    crate::galaxy_nav::visit_selected(&mut nav, galaxy);
                } else if let Some(star) = crate::galaxy_nav::picked_id(&field, id).or_else(|| {
                    // A list's stars are known without streaming them.
                    crate::galaxy_nav::mapped(galaxy, id).and_then(|(list, page)| {
                        crate::galaxy_nav::page_pick(&galaxy.systems[list], page)
                    })
                }) {
                    nav.select(Some(Selection::Star(star)));
                }
            }
            UiCommand::Jump { list: id } => {
                if let Some(index) = list(id) {
                    nav.select_list(index, galaxy);
                }
            }
            UiCommand::FindTask { list: id, task } => {
                if let Some(index) = list(id)
                    && let Some(planet) = galaxy.systems[index].task_planet(task)
                {
                    nav.select_task(index, planet, galaxy);
                }
            }
            UiCommand::Inspect { planet } => {
                if crate::objects::planet(galaxy, planet).is_some() {
                    let local = nav.system(galaxy).is_some_and(|system| {
                        system
                            .planets
                            .iter()
                            .any(|body| body.id == *planet && body.page() == nav.page)
                    });
                    nav.select(Some(Selection::Planet(planet.clone())));
                    if !local {
                        nav.following = false;
                    }
                }
            }
            UiCommand::InspectObject { inspection } => {
                let location = crate::ui::breadcrumb::trail(&nav, galaxy);
                let selected = crate::ui::breadcrumb::selection_trail(&nav, galaxy);
                if crate::ui::breadcrumb::holds(&nav, galaxy, inspection, &location, &selected) {
                    nav.inspection = Some(inspection.clone());
                }
            }
            UiCommand::ShowPage { page } => nav.show_page(*page, galaxy),
            UiCommand::SetPresentationMode { presentation_mode } => {
                nav.preset(*presentation_mode, galaxy)
            }
            UiCommand::VisitStar {
                id,
                presentation_mode,
            } => {
                if crate::galaxy_nav::visit_star(&mut nav, galaxy, &field, id)
                    && *presentation_mode != PresentationMode::Simulation
                {
                    nav.preset(*presentation_mode, galaxy);
                    if *presentation_mode == PresentationMode::Constellation {
                        let star = nav
                            .system(galaxy)
                            .and_then(|system| crate::galaxy_nav::page_pick(system, nav.page));
                        nav.select(star.map(Selection::Star));
                    }
                }
            }
            UiCommand::FlyToPlanet { planet } => {
                let local = nav
                    .system(galaxy)
                    .is_some_and(|system| system.planets.iter().any(|body| body.id == *planet));
                if crate::objects::planet(galaxy, planet).is_some()
                    && let Some(star) = crate::objects::parent_star(planet)
                    && (local || crate::galaxy_nav::visit_star(&mut nav, galaxy, &field, star))
                {
                    nav.select(Some(Selection::Planet(planet.clone())));
                    nav.focus_selected(galaxy);
                }
            }
            UiCommand::Follow => nav.focus_selected(galaxy),
            UiCommand::Up => {
                if nav.following && nav.selected_planet(galaxy).is_some() {
                    nav.frame(galaxy);
                } else if nav.selection.is_some() {
                    nav.select(None);
                } else if let Some(next) = crate::ui::breadcrumb::up(&nav, galaxy) {
                    queue.push_front(next);
                }
            }
            UiCommand::SetPaused { paused } => clock.0.paused = *paused,
            UiCommand::SetRate { rate } => clock.0.rate = *rate,
            UiCommand::SetMoveSpeed { speed } => {
                if speed.is_finite() {
                    nav.move_speed = speed.clamp(crate::scene::MIN_SPEED, crate::scene::MAX_SPEED);
                }
            }
            UiCommand::SetDisplay { option, on } => match option {
                Display::Exaggerate => nav.exaggerate = *on,
                Display::Orbits => nav.orbits = *on,
                Display::Labels => nav.labels = *on,
                Display::Markers => nav.markers = *on,
                Display::VelocityMarker => nav.velocity_marker = *on,
                Display::Realistic => nav.realistic = *on,
                Display::GalaxyGlow => nav.galaxy_glow = *on,
                Display::Wireframe => nav.wireframe = *on,
            },
            UiCommand::SetFov { degrees } => nav.set_fov(*degrees),
            UiCommand::SetMagnitude { magnitude } => nav.set_magnitude(*magnitude),
            UiCommand::SetExposure { stops } => nav.set_exposure(*stops),
            UiCommand::SetCompression { amount } => nav.set_compression(*amount),
            UiCommand::GalaxyReference { view } => galaxy_reference(&mut nav, *view),
            UiCommand::SetDebug { switch, on } => debug.set(*switch, *on),
        }
    }
}

#[cfg(test)]
mod tests;
