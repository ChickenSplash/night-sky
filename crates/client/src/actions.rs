//! Every panel action goes through `UiCommand`, whether it comes from egui or a
//! browser test script (`automation.rs`). Lists and tasks are named by ID, so a
//! data refresh that reorders them cannot redirect a click.
use crate::{Clock, Navigation, Selection, Universe, presentation::PresentationMode};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Message, Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    /// Select a streamed star by stable ID, or visit it on a second click.
    PickStar {
        id: String,
    },
    /// Select a known star by ID without travelling, even if already selected.
    SelectStar {
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
    /// Follow the selected planet or craft.
    Follow,
    /// Place a new ship above a body: a planet, star or barycentre ID, or a
    /// system's key; the selected body, else the visited system, without one.
    /// It starts in a circular orbit unless given a prograde speed or a
    /// velocity, both relative to the body and in km/s.
    SpawnShip {
        #[serde(default)]
        body: Option<String>,
        altitude_km: f64,
        #[serde(default)]
        speed_kms: Option<f64>,
        #[serde(default)]
        velocity_kms: Option<[f64; 3]>,
    },
    /// Remove a craft, or every craft without an ID.
    DespawnCraft {
        #[serde(default)]
        id: Option<String>,
    },
    /// Hold a craft's thrust, in m/s² in its system's axes: the selected
    /// craft, else the only one, without an ID.
    SetThrust {
        thrust: [f64; 3],
        #[serde(default)]
        id: Option<String>,
    },
    /// Body-axis drive or attitude order, targeting the selected or only ship.
    SteerCraft {
        order: crate::craft::Order,
        #[serde(default)]
        id: Option<String>,
        /// World step to apply from; absent means the next unsimulated step.
        #[serde(default)]
        tick: Option<i64>,
        /// Seconds from the ship's next step to apply from, instead of `tick`.
        #[serde(default)]
        in_s: Option<f64>,
        /// Seconds until the order's off order, for orders that have one.
        #[serde(default)]
        for_s: Option<f64>,
    },
    /// Select a craft by ID without moving the camera.
    SelectCraft {
        id: String,
    },
    /// Fly to a craft and follow it, visiting its system first.
    FlyToCraft {
        id: String,
    },
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
    /// Search from the camera and show the request in the object browser,
    /// replacing the one it shows (`search.rs`). In `ns search` text.
    Search {
        query: String,
    },
    /// Cancel and clear the object browser's request.
    ClearSearch,
    /// Debug-only search settings; either may be left out.
    SetSearch {
        #[serde(default)]
        threads: Option<usize>,
        #[serde(default)]
        work_cap_millions: Option<f64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    Orbits,
    Labels,
    Markers,
    VelocityMarker,
    Realistic,
    GalaxyGlow,
    Wireframe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GalaxyReference {
    Inside,
    FaceOn,
    EdgeOn,
}

impl UiCommand {
    /// Rejects values a typed command would otherwise clamp or drop. Panels
    /// and drags keep clamping in `Navigation`; only typed input checks.
    pub fn check_values(&self) -> Result<(), String> {
        fn within<T: PartialOrd + std::fmt::Display>(
            name: &str,
            value: T,
            range: std::ops::RangeInclusive<T>,
        ) -> Result<(), String> {
            if range.contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "{name} must be {} to {}, not {value}",
                    range.start(),
                    range.end()
                ))
            }
        }
        match self {
            UiCommand::SetRate { rate } => within("rate", *rate, 1.0..=crate::ui::hud::MAX_RATE),
            UiCommand::SetMoveSpeed { speed } => within(
                "speed",
                *speed,
                crate::scene::MIN_SPEED..=crate::scene::MAX_SPEED,
            ),
            UiCommand::SetFov { degrees } => {
                within("degrees", *degrees, crate::navigation::FOV_DEG)
            }
            UiCommand::SetMagnitude { magnitude } => {
                within("magnitude", *magnitude, crate::navigation::MAGNITUDE)
            }
            UiCommand::SetExposure { stops } => {
                within("stops", *stops, crate::render::exposure::STOPS)
            }
            UiCommand::SetCompression { amount } => within("amount", *amount, 0.0..=1.0),
            UiCommand::SpawnShip {
                altitude_km,
                speed_kms,
                velocity_kms,
                ..
            } => {
                within("altitude_km", *altitude_km, 0.0..=1e10)?;
                if let Some(speed) = speed_kms {
                    within("speed_kms", *speed, 0.0..=10_000.0)?;
                }
                if let Some(velocity) = velocity_kms
                    && !velocity
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= 10_000.0)
                {
                    return Err("velocity_kms must be three numbers, each within ±10000".into());
                }
                if speed_kms.is_some() && velocity_kms.is_some() {
                    return Err("Give speed_kms or velocity_kms, not both".into());
                }
                Ok(())
            }
            UiCommand::Search { query } => {
                space_simulation::search::Query::parse(query).and_then(|query| query.check())
            }
            UiCommand::SetSearch {
                threads,
                work_cap_millions,
            } => {
                if threads.is_none() && work_cap_millions.is_none() {
                    return Err("Give threads, work_cap_millions or both".into());
                }
                if let Some(threads) = threads {
                    within("threads", *threads, crate::search::THREADS)?;
                }
                if let Some(millions) = work_cap_millions {
                    within(
                        "work_cap_millions",
                        *millions,
                        crate::search::WORK_CAP_MILLIONS,
                    )?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Why `apply` would do nothing with this command right now, so typed and
    /// developer API commands can say so instead of being silently ignored.
    /// Keep in step with the conditions in `apply`.
    pub fn check(
        &self,
        galaxy: &space_shared::Galaxy,
        nav: &Navigation,
        field: &crate::stars::stream::Starfield,
        fleet: &crate::craft::Fleet,
    ) -> Result<(), String> {
        let list = |id: &str| {
            galaxy
                .systems
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| format!("Unknown list `{id}`. Lists are under `state lists`."))
        };
        let planet = |id: &str| {
            crate::objects::planet(galaxy, id)
                .map(|_| ())
                .ok_or_else(|| format!("Unknown planet `{id}`. Planet IDs are `<star>/<n>`."))
        };
        let star = |id: &str| {
            if crate::galaxy_nav::reachable(nav, galaxy, field, id) {
                Ok(())
            } else {
                Err(format!(
                    "Star `{id}` is not loaded, visited or a list's system"
                ))
            }
        };
        let craft = |id: &str| {
            fleet
                .get(id)
                .ok_or_else(|| format!("No craft `{id}`. Craft are under `state craft`."))
        };
        let visiting = || {
            nav.system(galaxy)
                .ok_or_else(|| "No system is visited. Visit a star first.".to_owned())
        };
        match self {
            UiCommand::PickStar { id } => {
                if nav.selected_star().is_some_and(|star| star.id == *id)
                    || crate::galaxy_nav::picked_id(field, id).is_some()
                    || crate::galaxy_nav::mapped(galaxy, id).is_some()
                {
                    Ok(())
                } else {
                    Err(format!("Star `{id}` is not loaded or a list's system"))
                }
            }
            UiCommand::SelectStar { id } => {
                crate::galaxy_nav::selectable_star(nav, galaxy, field, id)
                    .map(|_| ())
                    .ok_or_else(|| {
                        format!("Star `{id}` is not loaded, visited, mapped or found by search")
                    })
            }
            UiCommand::Jump { list: id } => list(id).map(|_| ()),
            UiCommand::FindTask { list: id, task } => {
                let index = list(id)?;
                galaxy.systems[index]
                    .task_planet(task)
                    .map(|_| ())
                    .ok_or_else(|| format!("List `{id}` has no task `{task}`"))
            }
            UiCommand::Inspect { planet: id } => planet(id),
            UiCommand::InspectObject { inspection } => {
                let location = crate::ui::breadcrumb::trail(nav, galaxy);
                let selected = crate::ui::breadcrumb::selection_trail(nav, galaxy, fleet.latest());
                if crate::ui::breadcrumb::holds(nav, galaxy, inspection, &location, &selected) {
                    Ok(())
                } else {
                    Err("That object is in neither the location nor the selection trail".into())
                }
            }
            UiCommand::ShowPage { page } => {
                let system = visiting()?;
                if system.pages().contains(page) {
                    Ok(())
                } else {
                    Err(format!(
                        "`{}` has no page {page}; it has {:?}",
                        system.name,
                        system.pages()
                    ))
                }
            }
            UiCommand::SetPresentationMode { .. } => visiting().map(|_| ()),
            UiCommand::VisitStar { id, .. } => star(id),
            UiCommand::FlyToPlanet { planet: id } => {
                planet(id)?;
                let local = nav
                    .system(galaxy)
                    .is_some_and(|system| system.planets.iter().any(|body| body.id == *id));
                match crate::objects::parent_star(id) {
                    _ if local => Ok(()),
                    Some(parent) => star(parent),
                    None => Err(format!("Planet `{id}` names no star")),
                }
            }
            UiCommand::Follow => {
                if nav.selected_planet(galaxy).is_some()
                    || crate::craft::followed(nav, galaxy, fleet, 0.0).is_some()
                {
                    Ok(())
                } else {
                    Err("Select a planet or craft in the visited system first".into())
                }
            }
            UiCommand::SpawnShip { body, .. } => {
                self.check_values()?;
                let body = spawn_body(body.as_deref(), nav, galaxy).ok_or_else(|| {
                    "Give a body, or select one or visit a system first".to_owned()
                })?;
                crate::craft::locate_body(galaxy, nav, field, &body).map(|_| ())
            }
            UiCommand::DespawnCraft { id } => match id {
                Some(id) => craft(id).map(|_| ()),
                None if fleet.craft.is_empty() => Err("There are no craft".into()),
                None => Ok(()),
            },
            UiCommand::SetThrust { id, thrust } => {
                let id = thrust_target(id.as_deref(), nav, fleet)?;
                let live = fleet.get(&id).ok_or("No craft")?;
                live.check(&crate::craft::Order::Thrust { thrust: *thrust })
            }
            UiCommand::SteerCraft {
                id,
                order,
                tick,
                in_s,
                for_s,
            } => {
                let id = thrust_target(id.as_deref(), nav, fleet)?;
                let live = fleet.get(&id).ok_or("No craft")?;
                match steer_steps(live.craft.tick, *tick, *in_s, *for_s)? {
                    (start, Some(end)) => live.check_timed_order(start, end, order).map(|_| ()),
                    (start, None) => live.check_order(start, order),
                }
            }
            UiCommand::SelectCraft { id } => craft(id).map(|_| ()),
            UiCommand::FlyToCraft { id } => craft(id).map(|_| ()),
            _ => self.check_values(),
        }
    }
}

/// The steps a ship order applies from and, when timed, ends at, from a
/// ship at step `now`.
fn steer_steps(
    now: i64,
    tick: Option<i64>,
    in_s: Option<f64>,
    for_s: Option<f64>,
) -> Result<(i64, Option<i64>), String> {
    use space_simulation::craft::voyage::steps;
    let start = match (tick, in_s) {
        (Some(_), Some(_)) => return Err("Give tick or in, not both".into()),
        (Some(tick), None) => tick,
        (None, Some(seconds)) => now + steps(seconds)?,
        (None, None) => now,
    };
    let end = for_s
        .map(steps)
        .transpose()?
        .map(|length| start.saturating_add(length));
    Ok((start, end))
}

/// The body a ship spawns at: the one given, else the selected planet or
/// star, else the planet the camera is at, else the visited system.
fn spawn_body(
    body: Option<&str>,
    nav: &Navigation,
    galaxy: &space_shared::Galaxy,
) -> Option<String> {
    if let Some(body) = body {
        return Some(body.to_owned());
    }
    match &nav.selection {
        Some(Selection::Planet(id)) => return Some(id.clone()),
        Some(Selection::Star(star)) => return Some(star.id.clone()),
        _ => {}
    }
    nav.location_planet.clone().or_else(|| {
        nav.system(galaxy)
            .map(|system| crate::objects::star_id(system, nav.page))
    })
}

/// The craft a thrust command steers: the one named, else the selected
/// craft, else the only craft.
fn thrust_target(
    id: Option<&str>,
    nav: &Navigation,
    fleet: &crate::craft::Fleet,
) -> Result<String, String> {
    let id = match (id, nav.selected_craft(), fleet.craft.as_slice()) {
        (Some(id), ..) | (None, Some(id), _) => id,
        (None, None, [only]) => &only.craft.id,
        (None, None, []) => return Err("There are no craft".into()),
        (None, None, _) => return Err("Give a craft ID or select a craft".into()),
    };
    fleet
        .get(id)
        .map(|live| live.craft.id.clone())
        .ok_or_else(|| format!("No craft `{id}`"))
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
    mut fleet: ResMut<crate::craft::Fleet>,
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
            UiCommand::SelectStar { id } => {
                if let Some(star) = crate::galaxy_nav::selectable_star(&nav, galaxy, &field, id) {
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
                let selected =
                    crate::ui::breadcrumb::selection_trail(&nav, galaxy, fleet.at(clock.0.seconds));
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
            UiCommand::Follow => {
                if crate::craft::followed(&nav, galaxy, &fleet, clock.0.seconds).is_some() {
                    nav.follow_craft();
                } else {
                    nav.focus_selected(galaxy);
                }
            }
            UiCommand::SpawnShip {
                body,
                altitude_km,
                speed_kms,
                velocity_kms,
            } => {
                let launch = match (speed_kms, velocity_kms) {
                    (Some(speed), _) => crate::craft::Launch::Speed(speed * 1_000.0),
                    (_, Some(velocity)) => {
                        crate::craft::Launch::Velocity(velocity.map(|v| v * 1_000.0))
                    }
                    _ => crate::craft::Launch::Circular,
                };
                let spawned = spawn_body(body.as_deref(), &nav, galaxy)
                    .ok_or_else(|| "nothing to spawn at".to_owned())
                    .and_then(|body| crate::craft::locate_body(galaxy, &nav, &field, &body))
                    .and_then(|(system, page, body)| {
                        let altitude = altitude_km * 1_000.0;
                        fleet.spawn(system, page, &body, altitude, launch, clock.0.seconds)
                    });
                match spawned {
                    Ok(id) => nav.select(Some(Selection::Craft(id))),
                    Err(err) => warn!(target: "craft", "Cannot spawn a ship: {err}"),
                }
            }
            UiCommand::DespawnCraft { id } => {
                fleet.despawn(id.as_deref());
                if nav
                    .selected_craft()
                    .is_some_and(|selected| fleet.get(selected).is_none())
                {
                    nav.select(None);
                }
            }
            UiCommand::SetThrust { thrust, id } => {
                let order = crate::craft::Order::Thrust { thrust: *thrust };
                if let Ok(id) = thrust_target(id.as_deref(), &nav, &fleet)
                    && let Some(live) = fleet.get_mut(&id)
                    && let Err(err) = live.voyage.order(live.craft.tick, order)
                {
                    warn!(target: "craft", "Thrust refused: {err}");
                }
            }
            UiCommand::SteerCraft {
                id,
                order,
                tick,
                in_s,
                for_s,
            } => {
                if let Ok(id) = thrust_target(id.as_deref(), &nav, &fleet)
                    && let Some(live) = fleet.get_mut(&id)
                    && let Err(err) =
                        steer_steps(live.craft.tick, *tick, *in_s, *for_s).and_then(|steps| {
                            match steps {
                                (start, Some(end)) => {
                                    live.voyage.timed_order(start, end, order.clone())
                                }
                                (start, None) => live.voyage.order(start, order.clone()),
                            }
                        })
                {
                    warn!(target: "craft", "Ship order refused: {err}");
                }
            }
            UiCommand::SelectCraft { id } => {
                if fleet.get(id).is_some() {
                    nav.select(Some(Selection::Craft(id.clone())));
                }
            }
            UiCommand::FlyToCraft { id } => {
                if let Some(live) = fleet.get(id) {
                    if live.star().is_empty() {
                        nav.visit = None;
                    } else {
                        nav.visit = Some(crate::Visit::Generated(Box::new(
                            live.field.system().clone(),
                        )));
                        nav.page = live.field.page();
                    }
                    nav.select(Some(Selection::Craft(id.clone())));
                    nav.follow_craft();
                }
            }
            UiCommand::Up => {
                if nav.following
                    && (nav.selected_planet(galaxy).is_some() || nav.selected_craft().is_some())
                {
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
            // `search::apply` runs these.
            UiCommand::Search { .. } | UiCommand::ClearSearch | UiCommand::SetSearch { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests;
