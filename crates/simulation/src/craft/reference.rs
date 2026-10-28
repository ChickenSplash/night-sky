//! The body a craft's orbit is described around: a planet, one star, or a
//! pair's barycentre. It is chosen from the system's hierarchy, not from
//! gravity, which always sums every star and planet: a barycentre is never
//! an extra gravitating body. It is a label for readouts, prediction and
//! pointing, and does not certify a stable orbit.
use super::*;
use crate::star::multiple::s_type_limit;

/// How far past its region a craft keeps its current reference, as a share
/// of the region's radius, so it does not flicker on a boundary.
pub const SWITCH_MARGIN: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Planet,
    Star,
    Barycentre,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub id: String,
    pub kind: ReferenceKind,
    /// GM of everything inside it in m³/s²: a pair's is all its stars'.
    pub gm: f64,
    /// Selected model reference radius, not certified ground; zero for a barycentre.
    pub radius_m: f64,
    pub state: State,
    /// Radius of the region around it where it is chosen: a planet's Hill
    /// sphere or a pair member's stable zone. Infinite for the whole system.
    pub reach_m: f64,
}

impl Field {
    /// Each pair member's stable zone in metres: where a planet could orbit
    /// it alone, against its partner's pull (Holman & Wiegert 1999).
    pub(super) fn zones(system: &StarSystem, page: u32) -> Vec<(String, f64)> {
        system
            .barycentres_on(page)
            .flat_map(|pair| {
                let masses = pair.members.each_ref().map(|m| system.member_mass(page, m));
                let total = masses[0] + masses[1];
                let a = pair.orbit.semi_major_au * crate::AU_METRES;
                let e = pair.orbit.eccentricity;
                (0..2).map(move |k| {
                    let zone = s_type_limit(masses[1 - k] / total, e) * a;
                    (pair.members[k].clone(), zone)
                })
            })
            .collect()
    }

    fn zone(&self, id: &str) -> Option<f64> {
        self.zones.iter().find(|(m, _)| m == id).map(|&(_, r)| r)
    }

    /// Stars and barycentres from the whole system down to `id`, then `id`
    /// itself when it is a planet.
    pub fn lineage(&self, id: &str) -> Vec<String> {
        match self.planets.iter().find(|(p, ..)| p.id == id) {
            Some((planet, ..)) => {
                let mut chain = self.system.lineage(self.page, &planet.host);
                chain.push(id.to_owned());
                chain
            }
            None => self.system.lineage(self.page, id),
        }
    }

    /// A planet, star or barycentre of this field as a reference.
    pub fn reference_by_id(&self, id: &str, seconds: f64) -> Option<Reference> {
        if let Some((planet, gm, radius, host)) = self.planets.iter().find(|(p, ..)| p.id == id) {
            return Some(Reference {
                id: id.to_owned(),
                kind: ReferenceKind::Planet,
                gm: *gm,
                radius_m: *radius,
                state: crate::planet_state(&self.system, planet, seconds),
                reach_m: hill(planet, *gm, *host),
            });
        }
        let state = crate::member_state(&self.system, self.page, id, seconds);
        let reach_m = self.zone(id).unwrap_or(f64::INFINITY);
        if let Some((_, gm, radius)) = self.stars.iter().find(|(s, ..)| s == id) {
            return Some(Reference {
                id: id.to_owned(),
                kind: ReferenceKind::Star,
                gm: *gm,
                radius_m: *radius,
                state,
                reach_m,
            });
        }
        self.system
            .barycentres_on(self.page)
            .any(|b| b.id == id)
            .then(|| Reference {
                id: id.to_owned(),
                kind: ReferenceKind::Barycentre,
                gm: self.system.member_mass(self.page, id) * SUN_GM,
                radius_m: 0.0,
                state,
                reach_m,
            })
    }

    /// The reference for a craft at `position`: the nearest planet whose
    /// Hill sphere holds it, else the innermost star or pair whose stable
    /// zone holds it, walking down from the whole system. `current`, and
    /// everything it lies within, is kept until the craft is past its region
    /// by `SWITCH_MARGIN`.
    pub fn select_reference(
        &self,
        current: Option<&str>,
        position: [f64; 3],
        seconds: f64,
    ) -> Option<Reference> {
        self.select_reference_floored(current, position, seconds, 0.0)
    }

    /// `select_reference`, with each planet's region never under
    /// `planet_radii` of its radii. Only the camera uses a floor: a craft
    /// that close to a hot Jupiter really does orbit the star.
    pub fn select_reference_floored(
        &self,
        current: Option<&str>,
        position: [f64; 3],
        seconds: f64,
        planet_radii: f64,
    ) -> Option<Reference> {
        let held = current.map(|id| self.lineage(id)).unwrap_or_default();
        let limit = |id: &str, radius: f64| {
            if held.iter().any(|h| h == id) {
                radius * (1.0 + SWITCH_MARGIN)
            } else {
                radius
            }
        };
        let planet = self
            .planets
            .iter()
            .filter_map(|(planet, gm, radius, host)| {
                let centre = crate::planet_position(&self.system, planet, seconds);
                let d = distance(centre, position);
                let reach = hill(planet, *gm, *host).max(radius * planet_radii);
                (d < limit(&planet.id, reach)).then_some((&planet.id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = planet {
            return self.reference_by_id(id, seconds);
        }
        let members = crate::orbit::member_positions(&self.system, self.page, seconds);
        let mut node = self.system.root(self.page)?;
        while let Some(pair) = self.system.barycentre(node) {
            let inner = pair
                .members
                .iter()
                .filter_map(|m| {
                    let d = distance(crate::orbit::find(&members, m)?, position);
                    (d < limit(m, self.zone(m)?)).then_some((m, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((member, _)) = inner else {
                break;
            };
            node = member;
        }
        self.reference_by_id(node, seconds)
    }
}

/// A planet's sphere of influence in metres: the region round it where it
/// is a craft's reference.
pub fn planet_reach_m(system: &StarSystem, planet: &Planet) -> f64 {
    hill(
        planet,
        planet.mass_earth * EARTH_SOLAR * SUN_GM,
        system.host_mass(planet),
    )
}

/// A planet's Hill sphere at its periapsis, in metres.
fn hill(planet: &Planet, gm: f64, host_solar: f64) -> f64 {
    let periapsis =
        planet.orbit.semi_major_au * (1.0 - planet.orbit.eccentricity) * crate::AU_METRES;
    periapsis * (gm / (3.0 * host_solar * SUN_GM)).cbrt()
}

#[cfg(test)]
mod tests;
