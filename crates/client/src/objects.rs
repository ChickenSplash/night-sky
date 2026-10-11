//! Stable inspection identities, independent of the scene's visited system.
use std::borrow::Cow;

use bevy::math::DVec3;
use serde::{Deserialize, Serialize};
use space_shared::{Galaxy, Planet, StarSystem};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectRef {
    Galaxy,
    List {
        id: String,
    },
    /// By its system's key, a star system as a whole: its star, or a multiple
    /// system's outermost barycentre. By a member ID (`key#B`), one star of a
    /// multiple system.
    Star {
        id: String,
    },
    /// A pair within a triple or quadruple system, such as its close pair.
    Barycentre {
        id: String,
    },
    Planet {
        id: String,
    },
}

/// Planet IDs encode their host, and so their system's key, including for
/// planets on list pages.
pub fn parent_star(id: &str) -> Option<&str> {
    Some(space_simulation::multiple::system_key(planet_host(id)?))
}

/// The star or barycentre a planet ID orbits.
pub fn planet_host(id: &str) -> Option<&str> {
    let (host, orbit) = id.rsplit_once('/')?;
    orbit.parse::<u32>().ok()?;
    Some(host)
}

/// Letters of a system's stars, such as "AB", or empty for a single star.
/// Works out only its stars, once per system.
pub fn system_letters(key: &str) -> String {
    static CACHE: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> =
        std::sync::Mutex::new(None);
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cache = cache.get_or_insert_with(Default::default);
    if cache.len() > 4_096 {
        cache.clear();
    }
    cache
        .entry(key.to_owned())
        .or_insert_with(|| {
            let stars = space_simulation::system_stars(key);
            if stars.len() > 1 {
                ('A'..).take(stars.len()).collect()
            } else {
                String::new()
            }
        })
        .clone()
}

/// A system's name with a member's letters: "TS 1-2-3 AB", or the name alone
/// for a single star.
pub fn member_label(name: &str, letters: &str) -> String {
    if letters.is_empty() {
        name.to_owned()
    } else {
        format!("{name} {letters}")
    }
}

pub fn star_id(system: &StarSystem, page: u32) -> String {
    system.page_star(page).map_or_else(
        || space_simulation::star_id(&system.id, page),
        |star| star.id.clone(),
    )
}

/// Resolve remote bodies without visiting them or relying on streamed meshes.
pub fn known_system<'a>(galaxy: &'a Galaxy, star: &str) -> Option<(&'a StarSystem, u32)> {
    if let Some((list, page)) = crate::galaxy_nav::mapped(galaxy, star) {
        return Some((&galaxy.systems[list], page));
    }
    for system in &galaxy.systems {
        if let Some(page) = system
            .pages()
            .into_iter()
            .find(|page| star_id(system, *page) == star)
        {
            return Some((system, page));
        }
    }
    None
}

pub fn system<'a>(galaxy: &'a Galaxy, star: &str) -> Option<(Cow<'a, StarSystem>, u32)> {
    if let Some((system, page)) = known_system(galaxy, star) {
        return Some((Cow::Borrowed(system), page));
    }
    // Inspection needs properties, not a spatial position. This placeholder
    // origin must never be used for travel; visits resolve the real star position.
    crate::galaxy_nav::generated(star, DVec3::ZERO).map(|system| (Cow::Owned(system), 0))
}

pub fn planet(galaxy: &Galaxy, id: &str) -> Option<Planet> {
    let (system, _) = system(galaxy, parent_star(id)?)?;
    system
        .planets
        .iter()
        .find(|planet| planet.id == id)
        .cloned()
}
