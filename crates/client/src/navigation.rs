//! Where the camera is going and what is selected: the destination that
//! `scene::CameraRig` flies the displayed camera towards.
pub mod flight;
pub mod galaxy_nav;
pub mod objects;
pub mod presentation;

use crate::{render, scene, ui};
use bevy::{math::DVec3, prelude::*};
use space_shared::{Galaxy, StarSystem};
use space_simulation::LY_METRES;

/// The star system the camera is at. List pages and generated systems are
/// both galactic stars; a list page adds its tasks.
#[derive(Clone, Debug, PartialEq)]
pub enum Visit {
    /// A list, by index into `Universe`, at `Navigation::page`.
    List(usize),
    Generated(Box<StarSystem>),
}

/// The one clicked object. A second click activates it: a star is visited, a
/// planet followed. A loaded system's star or barycentre marker selects
/// without a second action.
#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    /// Any star in the galaxy, by stable ID.
    Star(galaxy_nav::PickedStar),
    /// Stable planet ID, even when its parent system is not visited.
    Planet(String),
    /// A live craft, by its own ID (`craft::Fleet`).
    Craft(String),
    /// A pair of a multiple system, by member ID, such as `…#AB`.
    Barycentre(String),
}

impl Selection {
    pub fn planet(system: &StarSystem, index: usize) -> Self {
        Self::Planet(system.planets[index].id.clone())
    }

    pub fn object(&self) -> objects::ObjectRef {
        match self {
            Self::Star(star) => objects::ObjectRef::Star {
                id: star.id.clone(),
            },
            Self::Planet(id) => objects::ObjectRef::Planet { id: id.clone() },
            Self::Craft(id) => objects::ObjectRef::Craft { id: id.clone() },
            Self::Barycentre(id) => objects::ObjectRef::Barycentre { id: id.clone() },
        }
    }
}

/// What the camera moves with: the innermost neighbourhood it is in, set
/// each frame by `scene::locate_anchor`. Free movement is relative to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The `location_body`: a planet, or a star or pair of a multiple system.
    Body(String),
    Craft(String),
}

#[derive(Resource)]
pub struct Navigation {
    pub page: u32,
    pub presentation_mode: presentation::PresentationMode,
    pub viewport: Option<bevy::camera::Viewport>,
    /// The system the camera is anchored to: the one it is at, is flying to,
    /// or whose list is shown as a constellation. `None` in open space, where
    /// the camera is relative to home. See `location` for where it actually is.
    pub visit: Option<Visit>,
    /// Whether the camera is at the visited star, by physical distance. Set
    /// each frame by `scene::locate`.
    pub arrived: bool,
    /// Set by clicks, C or page controls. Selecting never moves the camera;
    /// visiting a star or following a planet does.
    pub selection: Option<Selection>,
    /// Which trail and object currently drive the floating inspector.
    pub inspection: Option<ui::breadcrumb::Inspection>,
    /// The body the camera is physically at, unrelated to selection or
    /// following: a planet, or a star or inner pair of a multiple system,
    /// by the rule that picks a craft's orbit reference. Never the whole
    /// system, which `arrived` already covers.
    pub location_body: Option<String>,
    /// What the camera moves with, by its own position alone.
    pub anchor: Option<Anchor>,
    /// Whether the camera keeps the selected object centred. Moving or
    /// looking ends it, not the camera's motion with its `anchor`.
    pub following: bool,
    pub target: DVec3,
    /// Authoritative free-flight eye in the current page's local frame.
    /// Avoids cancelling a distant target against the camera range.
    pub eye: Option<DVec3>,
    /// In open space, what `target` and `eye` are relative to, in light
    /// years from home: a followed or nearby craft's anchor, else one that
    /// moves with the camera (`scene::place_camera`), so they stay precise
    /// anywhere in the galaxy.
    pub galactic_origin_ly: DVec3,
    pub distance: f64,
    /// Selected free-flight speed in metres per real second, independent of zoom.
    pub move_speed: f64,
    /// Camera orientation in the current system's local plane. Keeping this as
    /// a quaternion lets mouse turns follow the screen axes even past the poles.
    pub rotation: Quat,
    /// Vertical field of view in degrees, within `FOV_DEG`.
    pub fov_deg: f32,
    /// Faintest apparent magnitude drawn as a star point, within
    /// `MAGNITUDE`. Fainter stars are part of the galaxy glow.
    pub magnitude: f64,
    /// Everything drawn over the scene that is not in it: markers, labels,
    /// paths and wireframes. Each of those has its own switch beneath this.
    pub overlays: bool,
    pub labels: bool,
    pub orbits: bool,
    /// Rings marking bodies too small to see. Hover and selection rings stay.
    pub markers: bool,
    pub velocity_marker: bool,
    /// The selected craft's predicted coast path.
    pub predicted_paths: bool,
    /// Lit and glowing bodies in HDR under the wireframes.
    pub realistic: bool,
    pub galaxy_glow: bool,
    /// Manual exposure in stops relative to the fixed reference sensitivity.
    pub exposure: f64,
    /// How far each body is exposed for itself: zero is one physical exposure,
    /// one shows every body correctly exposed (`render::resolve`).
    pub compression: f64,
    /// Wireframes over the realistic bodies. Always drawn otherwise.
    pub wireframe: bool,
    /// Systems found by the object browser's search, by key, with their
    /// positions in parsecs from the galactic centre: visitable without
    /// being streamed (`galaxy_nav::route`).
    pub charted: std::collections::HashMap<String, [f64; 3]>,
    pub notice: String,
    /// Animate the camera to the new target instead of cutting to it.
    pub fly: bool,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            page: 0,
            presentation_mode: presentation::PresentationMode::Simulation,
            viewport: None,
            visit: None,
            arrived: false,
            selection: None,
            inspection: None,
            location_body: None,
            anchor: None,
            galactic_origin_ly: DVec3::ZERO,
            following: false,
            target: DVec3::ZERO,
            eye: None,
            distance: 120.0 * LY_METRES,
            move_speed: 42.0 * LY_METRES,
            rotation: Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85),
            fov_deg: 50.0,
            magnitude: space_simulation::galaxy::population::VISIBLE_MAGNITUDE,
            overlays: true,
            labels: true,
            orbits: true,
            markers: true,
            velocity_marker: true,
            predicted_paths: true,
            realistic: true,
            galaxy_glow: true,
            exposure: 0.0,
            compression: 0.5,
            wireframe: true,
            charted: std::collections::HashMap::new(),
            notice: "Connecting to local API…".into(),
            fly: false,
        }
    }
}

impl Navigation {
    /// Whether an overlay is drawn: its own switch, and scene overlays on.
    pub fn overlay(&self, on: bool) -> bool {
        self.overlays && on
    }
}

/// Selectable vertical field of view, in degrees.
pub const FOV_DEG: std::ops::RangeInclusive<f32> = 30.0..=130.0;

/// Selectable star point magnitude limits. Each magnitude fainter draws
/// about four times the stars.
pub const MAGNITUDE: std::ops::RangeInclusive<f64> = 3.0..=10.5;

impl Navigation {
    /// Leave any system for open space, with `target` and `eye` then given
    /// relative to home.
    pub fn leave_for_home_frame(&mut self) {
        self.visit = None;
        self.galactic_origin_ly = DVec3::ZERO;
    }

    pub fn set_exposure(&mut self, stops: f64) {
        if stops.is_finite() {
            self.exposure = stops.clamp(
                *render::exposure::STOPS.start(),
                *render::exposure::STOPS.end(),
            );
        }
    }

    pub fn set_compression(&mut self, amount: f64) {
        if amount.is_finite() {
            self.compression = amount.clamp(0.0, 1.0);
        }
    }

    /// Clamped to `MAGNITUDE`, to a tenth.
    pub fn set_magnitude(&mut self, magnitude: f64) {
        if magnitude.is_finite() {
            self.magnitude =
                (magnitude.clamp(*MAGNITUDE.start(), *MAGNITUDE.end()) * 10.0).round() / 10.0;
        }
    }

    pub fn set_fov(&mut self, degrees: f32) {
        if degrees.is_finite() {
            self.fov_deg = degrees.clamp(*FOV_DEG.start(), *FOV_DEG.end());
        }
    }

    /// Tangent of half the vertical field of view, for projecting by hand.
    pub fn tan_half_fov(&self) -> f64 {
        (f64::from(self.fov_deg).to_radians() / 2.0).tan()
    }

    pub fn system<'a>(&'a self, lists: &'a Galaxy) -> Option<&'a StarSystem> {
        match self.visit.as_ref()? {
            Visit::List(index) => lists.systems.get(*index),
            Visit::Generated(system) => Some(system),
        }
    }

    pub fn selected_star(&self) -> Option<&galaxy_nav::PickedStar> {
        match &self.selection {
            Some(Selection::Star(star)) => Some(star),
            _ => None,
        }
    }

    pub fn selected_planet(&self, galaxy: &Galaxy) -> Option<usize> {
        match &self.selection {
            Some(Selection::Planet(id)) => self
                .system(galaxy)?
                .planets
                .iter()
                .position(|planet| planet.id == *id),
            _ => None,
        }
    }

    pub fn selected_barycentre(&self) -> Option<&str> {
        match &self.selection {
            Some(Selection::Barycentre(id)) => Some(id),
            _ => None,
        }
    }

    pub fn selected_craft(&self) -> Option<&str> {
        match &self.selection {
            Some(Selection::Craft(id)) => Some(id),
            _ => None,
        }
    }

    /// Replace the selection. Following carries over from planet to planet only.
    pub fn select(&mut self, selection: Option<Selection>) {
        if !matches!(selection, Some(Selection::Planet(_))) {
            self.following = false;
        }
        self.selection = selection;
        self.inspection = self
            .selection
            .as_ref()
            .map(|selection| ui::breadcrumb::Inspection {
                source: ui::breadcrumb::TrailSource::Selection,
                object: selection.object(),
            });
    }

    /// The system the camera is physically at, on `page`. `None` in open space,
    /// in constellation view and while a flight is still on its way in.
    pub fn location<'a>(&'a self, lists: &'a Galaxy) -> Option<&'a StarSystem> {
        self.system(lists).filter(|_| self.arrived)
    }

    /// The open list: the visited star's list, whether the camera is at it,
    /// flying to it or showing its constellation. It closes in open space.
    pub fn list(&self) -> Option<usize> {
        match self.visit {
            Some(Visit::List(index)) => Some(index),
            _ => None,
        }
    }

    /// Fly to a page's star, framing its planets in simulation mode.
    pub fn arrive(&mut self, visit: Visit, page: u32, lists: &Galaxy) {
        self.visit = Some(visit);
        self.page = page;
        self.frame(lists);
    }

    /// Frame the current page's planets in simulation mode.
    pub fn frame(&mut self, lists: &Galaxy) {
        let Some(distance) = self
            .system(lists)
            .map(|system| presentation::fit_distance(system, self.page))
        else {
            return;
        };
        self.fly = true;
        self.presentation_mode = presentation::PresentationMode::Simulation;
        self.rotation = Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85);
        self.selection = None;
        self.following = false;
        self.target = DVec3::ZERO;
        self.eye = None;
        self.distance = distance;
        self.move_speed = (distance * 0.35).clamp(scene::MIN_SPEED, scene::MAX_SPEED);
    }

    /// A list opens as a constellation regardless of the previous view.
    pub fn select_list(&mut self, index: usize, lists: &Galaxy) {
        let Some(system) = lists.systems.get(index) else {
            return;
        };
        let first_page = system.first_page();
        let star = galaxy_nav::page_pick(system, first_page);
        self.arrive(Visit::List(index), first_page, lists);
        self.preset(presentation::PresentationMode::Constellation, lists);
        self.selection = star.map(Selection::Star);
    }
}

/// A visit follows its star into a new snapshot: its list may move, a list may
/// gain a page on the visited star, or the list may go and leave the star bare.
/// Planet IDs name their star, so the selected planet survives each change.
pub fn follow_visited_star(nav: &mut Navigation, old: &Galaxy, new: &Galaxy) {
    let Some(system) = nav.system(old) else {
        return;
    };
    let Some(star) = system.page_star(nav.page).cloned() else {
        return;
    };
    let planet = nav
        .selected_planet(old)
        .and_then(|i| system.planets.get(i))
        .map(|p| p.id.clone());
    if let Some((list, page)) = galaxy_nav::mapped(new, &star.id) {
        nav.visit = Some(Visit::List(list));
        nav.page = page;
    } else if nav.list().is_some() {
        nav.visit = galaxy_nav::generated(&star.id, DVec3::from_array(star.position_ly))
            .map(|system| Visit::Generated(Box::new(system)));
        nav.page = 0;
    }
    if let Some(id) = planet {
        nav.selection = Some(Selection::Planet(id));
    }
    if nav.selected_planet(new).is_none() {
        nav.following = false;
    }
}
