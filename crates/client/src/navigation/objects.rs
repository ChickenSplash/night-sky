//! Stable inspection identities, independent of the scene's visited system.
use std::borrow::Cow;

use bevy::math::DVec3;
use space_shared::{Galaxy, Planet, StarSystem};

pub use space_simulation::objects::ObjectRef;

/// Select only, using the same commands as scene markers and browser rows.
pub fn selection_command(object: &ObjectRef) -> crate::actions::UiCommand {
    use crate::actions::UiCommand;
    match object {
        ObjectRef::Galaxy => UiCommand::SelectGalaxy,
        ObjectRef::List { id } => UiCommand::SelectList { id: id.clone() },
        ObjectRef::Star { id } => UiCommand::SelectStar { id: id.clone() },
        ObjectRef::Barycentre { id } => UiCommand::SelectBarycentre { id: id.clone() },
        ObjectRef::Planet { id } => UiCommand::Inspect { planet: id.clone() },
        ObjectRef::Craft { id } => UiCommand::SelectCraft { id: id.clone() },
    }
}

/// Planet IDs encode their host, and so their system's key, including for
/// planets on list pages.
pub fn parent_star(id: &str) -> Option<&str> {
    Some(space_simulation::star::multiple::system_key(planet_host(
        id,
    )?))
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
            let (stars, _) = space_simulation::system_stars(key);
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

/// A page's system as the breadcrumb names it: "System 2" on a list, as the
/// page controls number them, else the star's own name.
pub fn system_name(galaxy: &Galaxy, system: &StarSystem, page: u32) -> String {
    if galaxy.systems.iter().any(|list| list.id == system.id) {
        let number = system.pages().iter().position(|p| *p == page).unwrap_or(0) + 1;
        format!("System {number}")
    } else {
        system.name.clone()
    }
}

/// A star's or pair's label, such as "System 2 B": its system's name and
/// its letters within it.
pub fn page_member_label(galaxy: &Galaxy, system: &StarSystem, page: u32, id: &str) -> String {
    let letters = match system.barycentre(id) {
        Some(pair) => pair.component.as_str(),
        None => space_simulation::star::multiple::member_letters(id),
    };
    member_label(&system_name(galaxy, system, page), letters)
}

pub fn star_id(system: &StarSystem, page: u32) -> String {
    space_simulation::craft::voyage::page_star_id(system, page)
}

/// Resolve remote bodies without visiting them or relying on streamed meshes.
pub fn known_system<'a>(galaxy: &'a Galaxy, star: &str) -> Option<(&'a StarSystem, u32)> {
    space_simulation::craft::voyage::listed_page(galaxy, star)
}

pub fn system<'a>(galaxy: &'a Galaxy, star: &str) -> Option<(Cow<'a, StarSystem>, u32)> {
    if let Some((system, page)) = known_system(galaxy, star) {
        return Some((Cow::Borrowed(system), page));
    }
    // Inspection needs properties, not a spatial position. This placeholder
    // origin must never be used for travel; visits resolve the real star position.
    crate::navigation::galaxy_nav::generated(star, DVec3::ZERO)
        .map(|system| (Cow::Owned(system), 0))
}

pub fn planet(galaxy: &Galaxy, id: &str) -> Option<Planet> {
    let (system, _) = system(galaxy, parent_star(id)?)?;
    system
        .planets
        .iter()
        .find(|planet| planet.id == id)
        .cloned()
}
