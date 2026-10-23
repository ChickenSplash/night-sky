//! Wireframe bodies, orbits and constellation links drawn with gizmos, and
//! the lights and markers gathered while drawing them.
use super::{
    camera::{CameraPose, SpaceCamera},
    frames::{page_frame, page_offsets, spin_basis},
    location::{LEAVE_FACTOR, WIDE_METRES, body_range},
};
use crate::presentation::{PresentationMode, TableLayout};
use crate::render::{self, Illumination, Lights, MAX_LIGHTS, Photometry, PlanetLight, StarLight};
use crate::{Clock, Navigation, Universe};
use bevy::{
    math::{DQuat, DVec3},
    prelude::*,
};
use space_shared::{Orbit, Star, StarSystem};
use space_simulation::{
    SUN_RADIUS_METRES, member_position, planet_position, position_at_eccentric_anomaly,
};
use std::f64::consts::TAU;

/// A label and hit target: a planet's, craft's, or a loaded system's star
/// or barycentre. Every other star is picked from the streamed galaxy.
#[derive(Clone)]
pub struct Marker {
    pub point: Vec2,
    /// Distance from the camera in render units, only to order markers.
    pub depth: f32,
    pub radius: f32,
    pub label: String,
    pub colour: [u8; 3],
    pub object: Marked,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Marked {
    /// Index into the visited system's planets.
    Planet(usize),
    /// A craft's ID.
    Craft(String),
    /// A star of the loaded page, by member ID.
    Star(String),
    /// A pair of the loaded page, the whole system's included, by member ID.
    Barycentre(String),
}

/// Barycentres have nothing to see, so their markers are drawn in one colour.
pub const BARYCENTRE_COLOUR: [u8; 3] = [175, 195, 215];

/// What a loaded page marks: each star drawn, and each pair, the whole
/// system's included, except in the table, which gathers them at the primary.
pub(super) fn member_markers(system: &StarSystem, page: u32, table: bool) -> Vec<Marked> {
    let stars = system
        .stars_on(page)
        .take(if table { 1 } else { usize::MAX })
        .map(|star| Marked::Star(star.id.clone()));
    let pairs = system
        .barycentres_on(page)
        .filter(|_| !table)
        .map(|pair| Marked::Barycentre(pair.id.clone()));
    stars.chain(pairs).collect()
}

/// Craft are drawn in one colour, apart from any task's.
pub const CRAFT_COLOUR: [u8; 3] = [140, 225, 255];

#[derive(Resource, Default)]
pub struct Markers(pub Vec<Marker>);

fn colour(rgb: [u8; 3]) -> Color {
    Color::srgb_u8(rgb[0], rgb[1], rgb[2])
}

pub(super) fn orbit_point(orbit: &Orbit, step: usize, steps: usize) -> DVec3 {
    DVec3::from_array(position_at_eccentric_anomaly(
        orbit,
        step as f64 / steps as f64 * TAU,
    ))
}

const ORBIT: Color = Color::srgb(0.075, 0.17, 0.25);
/// A selected craft's predicted coast path, and the same once out of date.
const TRAJECTORY: Color = Color::srgb(0.45, 0.8, 0.95);
const TRAJECTORY_STALE: Color = Color::srgb(0.2, 0.32, 0.38);
/// Where its orbit reference would change, and where it would hit a body.
const TRAJECTORY_REFERENCE: Color = Color::srgb(0.95, 0.7, 0.3);
const TRAJECTORY_IMPACT: Color = Color::srgb(0.95, 0.3, 0.25);
/// Bodies below this on-screen radius, in logical pixels, draw as a single ring.
const RING_PIXELS: f32 = 6.0;
const LINK: Color = Color::srgb(0.12, 0.3, 0.4);

/// Wide-view star sizes follow eye distance, never the camera's zoom range.
pub(super) fn star_display_radius(physical: f64, away: f64, (table, wide): (bool, bool)) -> f64 {
    if table {
        physical
    } else if wide {
        (away * 0.004).max(physical)
    } else {
        physical
    }
}

/// Lines are cut where they come closer to the camera plane than this share of
/// their ends' distance: far off-screen, yet clear of f32 rounding.
const CLIP_SHARE: f64 = 1e-4;

/// A line through `points` cut to the part in front of the camera, as
/// separate strips. Bevy clips lines at the near plane in f32 on the GPU; for
/// ends many orders of magnitude beyond it (a star's rings or a planet's
/// orbit seen side-on) rounding can put the cut end behind the camera, and
/// its line then crosses the opposite side of the screen.
pub(super) fn clip_strip(
    points: impl IntoIterator<Item = Vec3>,
    eye: DVec3,
    forward: DVec3,
) -> Vec<Vec<Vec3>> {
    let mut strips = Vec::new();
    let mut run = Vec::new();
    let mut points = points.into_iter().map(|p| p.as_dvec3());
    let Some(mut a) = points.next() else {
        return strips;
    };
    for b in points {
        let margin = CLIP_SHARE * (a - eye).length().max((b - eye).length());
        let [da, db] = [a, b].map(|p| (p - eye).dot(forward) - margin);
        let cut = || a.lerp(b, da / (da - db)).as_vec3();
        if da >= 0.0 {
            if run.is_empty() {
                run.push(a.as_vec3());
            }
            if db >= 0.0 {
                run.push(b.as_vec3());
            } else {
                run.push(cut());
                strips.push(std::mem::take(&mut run));
            }
        } else if db >= 0.0 {
            run = vec![cut(), b.as_vec3()];
        }
        a = b;
    }
    if run.len() > 1 {
        strips.push(run);
    }
    strips
}

/// One frame's projection from f64 metres relative to the current star into render
/// units around the floating-origin camera.
struct Canvas<'a> {
    camera: &'a Camera,
    view: &'a GlobalTransform,
    origin: Vec3,
    scale: f64,
    pose: &'a CameraPose,
    nav: &'a Navigation,
    galaxy: &'a space_shared::Galaxy,
    system: &'a StarSystem,
    /// Plane of the page on display.
    frame: DQuat,
    seconds: f64,
    photometry: &'a Photometry,
    frame_ly: DVec3,
    /// The prototype's air, for the selected planet.
    air: render::atmosphere::Atmosphere,
}

impl Canvas<'_> {
    /// Subtracts in f64 before conversion, so small local positions are never added
    /// to an interstellar f32 coordinate, including while following a moving planet.
    fn world(&self, galactic: DVec3) -> Vec3 {
        self.origin + ((galactic - self.pose.eye) / self.scale).as_vec3()
    }

    /// A line strip, cut in front of the camera (`clip_strip`).
    fn strip(&self, gizmos: &mut Gizmos, points: impl IntoIterator<Item = Vec3>, tint: Color) {
        let eye = self.view.translation().as_dvec3();
        for strip in clip_strip(points, eye, self.view.forward().as_dvec3()) {
            gizmos.linestrip(strip, tint);
        }
    }

    fn units(&self, metres: f64) -> f32 {
        (metres / self.scale) as f32
    }

    fn screen(&self, point: Vec3) -> Option<Vec2> {
        self.camera.world_to_viewport(self.view, point).ok()
    }

    fn depth(&self, point: Vec3) -> f32 {
        point.distance(self.view.translation())
    }

    /// On-screen radius in logical pixels, or `None` when the centre is off-screen.
    /// Measured as if the body sat at the screen centre: perspective stretches
    /// bodies towards the edges, which would switch their line density there.
    fn pixels(&self, centre: Vec3, radius: f32) -> Option<f32> {
        self.screen(centre)?;
        let eye = self.view.translation();
        let ahead = eye + self.view.forward() * centre.distance(eye);
        let point = self.screen(ahead)?;
        let edge = self.screen(ahead + self.view.right() * radius)?;
        Some(edge.distance(point))
    }

    /// Render units per logical pixel at a point's depth, on or off screen.
    fn per_pixel(&self, centre: Vec3) -> Option<f32> {
        let eye = self.view.translation();
        let probe = centre.distance(eye) * 0.01;
        let ahead = eye + self.view.forward() * centre.distance(eye);
        let pixels = self
            .screen(ahead)?
            .distance(self.screen(ahead + self.view.right() * probe)?);
        (pixels > 0.0).then(|| probe / pixels)
    }

    /// Draws a wireframe body and returns its on-screen radius.
    /// `basis` orients the latitude rings, so bodies share their system's plane.
    /// `flat` projects the sphere onto its equatorial plane, as seen from directly
    /// above. Used for a star beside a close top-down camera, where its upper rings
    /// would otherwise reach past the camera.
    fn sphere(
        &self,
        gizmos: &mut Gizmos,
        centre: Vec3,
        radius: f32,
        tint: Color,
        basis: DQuat,
        flat: bool,
    ) -> Option<f32> {
        use std::f32::consts::{PI, TAU};
        let pixels = self.pixels(centre, radius);
        let basis = basis.as_quat();
        // Line density follows on-screen size, so small bodies stay legible instead of
        // collapsing into a solid blob of overlapping rings.
        let (latitudes, longitudes, steps) = match pixels.unwrap_or(0.0) {
            p if p < RING_PIXELS => (0, 0, 24),
            p if p < 24.0 => (1, 3, 24),
            p if p < 90.0 => (2, 6, 40),
            p => (4, 12, (p as usize / 4).clamp(96, 1024)),
        };
        let point = |v: Vec3| centre + basis * if flat { v.with_y(0.0) } else { v } * radius;
        if latitudes == 0 {
            let (right, up) = if flat {
                (basis * Vec3::X, basis * Vec3::Z)
            } else {
                (self.view.right().as_vec3(), self.view.up().as_vec3())
            };
            self.strip(
                gizmos,
                (0..=steps).map(|i| {
                    let a = i as f32 / steps as f32 * TAU;
                    centre + (right * a.cos() + up * a.sin()) * radius
                }),
                tint,
            );
            return pixels;
        }
        let ring =
            |phi: f32, a: f32| Vec3::new(phi.cos() * a.cos(), phi.sin(), phi.cos() * a.sin());
        let band = PI / (2 * latitudes + 2) as f32;
        for latitude in -latitudes..=latitudes {
            let phi = latitude as f32 * band;
            self.strip(
                gizmos,
                (0..=steps).map(|i| point(ring(phi, i as f32 / steps as f32 * TAU))),
                tint,
            );
        }
        for longitude in 0..longitudes {
            let a = longitude as f32 * PI / longitudes as f32;
            self.strip(
                gizmos,
                (0..=steps).map(|i| point(ring(i as f32 / steps as f32 * TAU, a))),
                tint,
            );
        }
        pixels
    }

    /// A dot about two pixels across: a ring too small to have a hole.
    fn dot(&self, gizmos: &mut Gizmos, centre: Vec3, tint: Color) {
        use std::f32::consts::TAU;
        let Some(per_pixel) = self.per_pixel(centre) else {
            return;
        };
        let (right, up) = (self.view.right().as_vec3(), self.view.up().as_vec3());
        self.strip(
            gizmos,
            (0..=8).map(|i| {
                let a = i as f32 / 8.0 * TAU;
                centre + (right * a.cos() + up * a.sin()) * 0.5 * per_pixel
            }),
            tint,
        );
    }

    /// A body's wireframe, unless realistic bodies are drawn without them, and its
    /// on-screen radius. Over a realistic body the wireframe sits just outside its
    /// surface, so the body hides the far side.
    #[allow(clippy::too_many_arguments)]
    fn outline(
        &self,
        gizmos: &mut Gizmos,
        centre: Vec3,
        radius: f32,
        tint: Color,
        basis: DQuat,
        flat: bool,
    ) -> Option<f32> {
        match (self.nav.realistic, self.nav.overlay(self.nav.wireframe)) {
            // Without realistic lighting a small body is a dot: its marker is the ring.
            (false, _) => match self.pixels(centre, radius) {
                Some(p) if p < RING_PIXELS => {
                    self.dot(gizmos, centre, tint);
                    Some(p)
                }
                _ => self.sphere(gizmos, centre, radius, tint, basis, flat),
            },
            // A small realistic body is its own point: a ring would only hide it.
            (true, true) => match self.pixels(centre, radius) {
                Some(p) if p < RING_PIXELS => Some(p),
                _ => self.sphere(gizmos, centre, radius * 1.01, tint, basis, flat),
            },
            (true, false) => self.pixels(centre, radius),
        }
    }

    /// Joins the current list's page stars where they already are, like a star
    /// chart: each star links to its nearest neighbour in a minimum spanning tree.
    fn constellation(&self, gizmos: &mut Gizmos) {
        let stars = page_offsets(self.system, self.pose.page);
        let Some(&first) = stars.first() else {
            return;
        };
        let mut joined = vec![false; stars.len()];
        let mut nearest: Vec<_> = stars.iter().map(|s| ((*s - first).length(), 0)).collect();
        joined[0] = true;
        for _ in 1..stars.len() {
            let Some(next) = (0..stars.len())
                .filter(|&i| !joined[i])
                .min_by(|&a, &b| nearest[a].0.total_cmp(&nearest[b].0))
            else {
                break;
            };
            joined[next] = true;
            self.strip(
                gizmos,
                [self.world(stars[nearest[next].1]), self.world(stars[next])],
                LINK,
            );
            for i in 0..stars.len() {
                let distance = (stars[i] - stars[next]).length();
                if !joined[i] && distance < nearest[i].0 {
                    nearest[i] = (distance, next);
                }
            }
        }
    }

    /// Where a star or barycentre of the page is drawn, in its orbital-plane
    /// axes: on its orbit, or gathered at the primary in the table.
    fn member(&self, id: &str) -> DVec3 {
        let orbital = DVec3::from_array(member_position(
            self.system,
            self.pose.page,
            id,
            self.seconds,
        ));
        orbital * (1.0 - self.pose.layout)
    }

    /// The visited page's stars, within `body_range`, with a marker for each
    /// star and pair. Every other star, including the rest of its list, is a
    /// streamed galactic point. The table shows only the primary, its largest.
    fn stars(&self, gizmos: &mut Gizmos, lights: &mut Lights, markers: &mut Vec<Marker>) {
        let (pose, system) = (self.pose, self.system);
        let table = pose.layout > 0.5;
        let stars: Vec<_> = system.stars_on(pose.page).collect();
        if stars.is_empty() {
            self.star(gizmos, lights, None, DVec3::ZERO);
        }
        let mut drawn = Vec::new();
        for (index, body) in stars.into_iter().enumerate() {
            if index == 0 || !table {
                let pixels = self.star(gizmos, lights, Some(body), self.member(&body.id));
                drawn.push((body, pixels));
            }
        }
        for object in member_markers(system, pose.page, table) {
            let (Marked::Star(id) | Marked::Barycentre(id)) = &object else {
                continue;
            };
            let star = drawn.iter().find(|(body, _)| body.id == *id);
            let centre = self.world(self.frame * self.member(id));
            let Some(point) = self.screen(centre) else {
                continue;
            };
            markers.push(Marker {
                point,
                depth: self.depth(centre),
                radius: star.and_then(|(_, pixels)| *pixels).unwrap_or(0.0).max(5.0),
                label: crate::objects::page_member_label(self.galaxy, system, pose.page, id),
                colour: star.map_or(BARYCENTRE_COLOUR, |(body, _)| body.colour),
                object,
            });
        }
    }

    /// Draws a star and returns its on-screen radius.
    fn star(
        &self,
        gizmos: &mut Gizmos,
        lights: &mut Lights,
        body: Option<&Star>,
        local: DVec3,
    ) -> Option<f32> {
        let (nav, pose) = (self.nav, self.pose);
        let wide = pose.distance > WIDE_METRES && pose.eye.length() > WIDE_METRES;
        let table = pose.layout > 0.5;
        let galactic = self.frame * local;
        let centre = self.world(galactic);
        let physical = body.map_or(SUN_RADIUS_METRES, |b| b.radius_solar * SUN_RADIUS_METRES);
        let tint = body.map_or([255, 192, 108], |b| b.colour);
        let basis = match body {
            Some(b) => spin_basis(self.frame, &b.spin, self.seconds),
            None => self.frame,
        };
        let away = (galactic - pose.eye).length();
        let radius = star_display_radius(physical, away, (table, wide));
        if nav.realistic
            && let Some(body) = body
            && let Some(per_pixel) = self.per_pixel(centre)
        {
            let surface = self.photometry.surface_radiance(body);
            // An enlarged star dims to keep its light. From afar it keeps its
            // true angular size.
            let (disc, radiance) = if wide && !table {
                (physical, surface)
            } else {
                (radius, surface * (physical / radius).powi(2))
            };
            let disc = self.units(disc);
            lights.stars.push(StarLight {
                centre,
                radius: disc,
                pixels: disc / per_pixel,
                per_pixel,
                radiance,
                kelvin: body.temperature_k,
                spin: basis.as_quat(),
                granules: render::granules(body),
            });
        }
        self.outline(
            gizmos,
            centre,
            self.units(radius),
            colour(tint),
            basis,
            table,
        )
    }

    /// Each star's orbit around its pair's barycentre, which itself moves.
    fn star_orbits(&self, gizmos: &mut Gizmos) {
        let (page, system) = (self.pose.page, self.system);
        for pair in system.barycentres_on(page) {
            let centre = self.member(&pair.id);
            let masses = pair.members.each_ref().map(|m| system.member_mass(page, m));
            let total = masses[0] + masses[1];
            for share in [-masses[1] / total, masses[0] / total] {
                self.strip(
                    gizmos,
                    (0..=192).map(|step| {
                        let local = centre + orbit_point(&pair.orbit, step, 192) * share;
                        self.world(self.frame * local)
                    }),
                    ORBIT,
                );
            }
        }
    }

    /// Light reaching a planet at `orbital` (drawn at `drawn`, both in the
    /// page's axes) from each star, brightest first.
    fn illumination(&self, orbital: DVec3, drawn: DVec3) -> Vec<Illumination> {
        let page = self.pose.page;
        let mut light: Vec<Illumination> = self
            .system
            .stars_on(page)
            .map(|star| {
                let at =
                    DVec3::from_array(member_position(self.system, page, &star.id, self.seconds));
                Illumination {
                    // Lit from the stars even in the table, which is only a layout.
                    towards: (self.frame * (self.member(&star.id) - drawn))
                        .normalize_or_zero()
                        .as_vec3(),
                    flux: space_simulation::light::flux(
                        self.photometry.visual_luminosity(star),
                        (at - orbital).length(),
                    ),
                    kelvin: star.temperature_k,
                }
            })
            .collect();
        light.sort_by(|a, b| b.flux.total_cmp(&a.flux));
        light.truncate(MAX_LIGHTS);
        light
    }

    /// The destination page's planets, blending between running orbits and the
    /// table row while the camera flies. Orbital radii are never presentation-scaled.
    fn planets(
        &self,
        gizmos: &mut Gizmos,
        seconds: f64,
        markers: &mut Vec<Marker>,
        lights: &mut Lights,
    ) {
        let (nav, pose, system) = (self.nav, self.pose, self.system);
        let layout = TableLayout::new(system, pose.page);
        let place = |local: DVec3| self.frame * local;
        if nav.overlay(nav.orbits)
            && nav.presentation_mode == PresentationMode::Simulation
            && pose.layout <= 0.5
        {
            self.star_orbits(gizmos);
            for planet in system.on_page(pose.page) {
                let host = self.member(&planet.host);
                self.strip(
                    gizmos,
                    (0..=192).map(|step| {
                        self.world(place(host + orbit_point(&planet.orbit, step, 192)))
                    }),
                    ORBIT,
                );
            }
        }
        for (index, planet) in system.planets.iter().enumerate() {
            if planet.page() != pose.page {
                continue;
            }
            let orbital = DVec3::from_array(planet_position(system, planet, seconds));
            let local = orbital.lerp(layout.position(planet), pose.layout);
            let galactic = place(local);
            let centre = self.world(galactic);
            let radius = planet.radius_km * 1000.0;
            let tint = colour(planet.colour());
            let basis = spin_basis(self.frame, &planet.spin, seconds);
            if nav.realistic
                && let Some(per_pixel) = self.per_pixel(centre)
            {
                let disc = self.units(radius);
                // Prototype: the debug air on the selected planet only.
                let selected = matches!(&nav.selection,
                    Some(crate::Selection::Planet(id)) if *id == planet.id);
                lights.planets.push(PlanetLight {
                    centre,
                    radius: disc,
                    pixels: disc / per_pixel,
                    per_pixel,
                    light: self.illumination(orbital, local),
                    surface: space_simulation::light::surface(&planet.class),
                    air: selected.then(|| self.air.scaled(radius)),
                });
            }
            let pixels = self.outline(gizmos, centre, self.units(radius), tint, basis, false);
            if let Some(point) = self.screen(centre) {
                markers.push(Marker {
                    point,
                    depth: self.depth(centre),
                    radius: pixels.unwrap_or(6.0).max(5.0),
                    label: planet.label().to_owned(),
                    colour: planet.colour(),
                    object: Marked::Planet(index),
                });
            }
        }
    }
}

impl Canvas<'_> {
    /// Each craft of the page as a test hull at its true size, turned to
    /// its attitude, and its marker. Only orbital positions have craft.
    fn craft(&self, gizmos: &mut Gizmos, fleet: &crate::craft::Fleet, markers: &mut Vec<Marker>) {
        if self.pose.layout > 0.0 {
            return;
        }
        for live in &fleet.craft {
            let state = live.state_at(self.seconds);
            let offset = (live.origin_ly() - self.frame_ly) * space_simulation::LY_METRES;
            let relative = self.pose.craft_relative(&live.craft.id).unwrap_or_else(|| {
                offset + live.basis() * DVec3::from_array(state.position) - self.pose.eye
            });
            let centre = self.origin + (relative / self.scale).as_vec3();
            let half = self.units(crate::craft::SHIP_RADIUS_M);
            let turn = (live.basis() * live.orientation_at(self.seconds)).as_quat();
            let pixels = self.pixels(centre, half);
            if pixels.is_some_and(|p| p >= 1.0) {
                let vertex = |i: usize| {
                    centre
                        + turn
                            * DVec3::from_array(crate::craft::HULL_VERTICES[i]).as_vec3()
                            * self.units(1.0)
                };
                for (a, b) in crate::craft::HULL_EDGES {
                    self.strip(gizmos, [vertex(a), vertex(b)], colour(CRAFT_COLOUR));
                }
            }
            if let Some(point) = self.screen(centre) {
                markers.push(Marker {
                    point,
                    depth: self.depth(centre),
                    radius: pixels.unwrap_or(0.0).max(5.0),
                    label: live.craft.name.clone(),
                    colour: CRAFT_COLOUR,
                    object: Marked::Craft(live.craft.id.clone()),
                });
            }
            if self.nav.overlay(self.nav.predicted_paths)
                && self.nav.selection == Some(crate::Selection::Craft(live.craft.id.clone()))
                && live.star() == crate::objects::star_id(self.system, self.pose.page)
            {
                self.trajectory(gizmos, live);
            }
        }
    }

    /// The shown coast prediction still ahead, from the craft, relative to
    /// its reference anchored where that reference is now, with diamonds at
    /// its apsides and reference changes. Dimmer once the craft's inputs or
    /// reference have changed since it was made.
    fn trajectory(&self, gizmos: &mut Gizmos, live: &crate::craft::Live) {
        use crate::craft::PathMark;
        let Some(path) = crate::craft::predicted_path(live, self.seconds) else {
            return;
        };
        let place = |local: [f64; 3]| self.world(self.frame * DVec3::from_array(local));
        let tint = if path.current {
            TRAJECTORY
        } else {
            TRAJECTORY_STALE
        };
        self.strip(gizmos, path.points.iter().map(|&p| place(p)), tint);
        for (local, mark) in path.marks {
            let colour = match mark {
                PathMark::Apsis => tint,
                PathMark::Reference => TRAJECTORY_REFERENCE,
                PathMark::Impact => TRAJECTORY_IMPACT,
            };
            let centre = place(local);
            let Some(per_pixel) = self.per_pixel(centre) else {
                continue;
            };
            let [right, up] = [self.view.right(), self.view.up()].map(|v| *v * per_pixel * 5.0);
            let corners = [
                centre + up,
                centre + right,
                centre - up,
                centre - right,
                centre + up,
            ];
            self.strip(gizmos, corners, colour);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut gizmos: Gizmos,
    universe: Res<Universe>,
    nav: Res<Navigation>,
    clock: Res<Clock>,
    pose: Res<CameraPose>,
    camera: Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
    mut markers: ResMut<Markers>,
    light: (
        ResMut<Lights>,
        Res<Photometry>,
        Res<render::atmosphere::AtmosphereTuning>,
    ),
    fleet: Res<crate::craft::Fleet>,
    rig: Res<super::CameraRig>,
) {
    let (mut lights, photometry, tuning) = light;
    markers.0.clear();
    lights.clear();
    let Ok((camera, view)) = camera.single() else {
        return;
    };
    lights.eye = view.translation();
    let visited = nav.system(&universe.0);
    let Some(system) = visited.or_else(|| fleet.craft.first().map(|live| live.field.system()))
    else {
        return;
    };
    // During a page-to-page flight the camera may still be near the departure
    // star. Do not draw the destination's AU-scale orbits from interstellar range.
    let planets_shown = visited.is_some() && body_range(&nav, system, &pose).is_some();
    let canvas = Canvas {
        camera,
        view,
        origin: view.translation(),
        scale: pose.render_scale(),
        pose: &pose,
        nav: &nav,
        galaxy: &universe.0,
        system,
        frame: page_frame(system, pose.page),
        seconds: clock.0.seconds,
        photometry: &photometry,
        frame_ly: rig.frame_ly,
        air: tuning.air.atmosphere(),
    };
    let markers = &mut markers.0;
    if nav.overlays && nav.presentation_mode == PresentationMode::Constellation {
        canvas.constellation(&mut gizmos);
    }
    if let Some(range) = body_range(&nav, system, &pose) {
        // Zero where the camera arrives, and so where galactic points become
        // bodies, full at half that distance.
        let arrive = range / LEAVE_FACTOR;
        lights.streaks = ((arrive - pose.eye.length()) / (0.5 * arrive)).clamp(0.0, 1.0) as f32;
    }
    let mut stars = Vec::new();
    if planets_shown {
        canvas.stars(&mut gizmos, &mut lights, &mut stars);
        canvas.planets(&mut gizmos, clock.0.seconds, markers, &mut lights);
    }
    canvas.craft(&mut gizmos, &fleet, markers);
    markers.extend(stars);
}
