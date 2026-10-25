//! Optional list/task placement on otherwise independently generated systems.
use super::generation::{bodies, planet_count};
use crate::seed::stable_hash;
use crate::star::multiple::{Bodies, designation};
use space_shared::{Galaxy, Planet, StarSystem, Task};
use std::collections::BTreeMap;

/// ID of a list page's star, which seeds the star and its planets.
pub fn star_id(list_id: &str, page: u32) -> String {
    format!("{list_id}:page-{page}")
}

/// Number of planets, and so of task slots, in a page's system.
pub fn page_capacity(list_id: &str, page: u32) -> u32 {
    planet_count(&star_id(list_id, page))
}

fn assigned_star(system: &StarSystem, page: u32) -> String {
    system
        .page_star(page)
        .map_or_else(|| star_id(&system.id, page), |s| s.id.clone())
}

/// Each page's generated bodies, worked out once while placing a list.
type Generated = BTreeMap<u32, Bodies>;

fn generated<'a>(system: &StarSystem, cache: &'a mut Generated, page: u32) -> &'a Bodies {
    cache
        .entry(page)
        .or_insert_with(|| bodies(&assigned_star(system, page)))
}

fn slot_page(system: &StarSystem, cache: &mut Generated, slot: u32) -> (u32, u32) {
    let (mut page, mut first) = (0, 0);
    loop {
        let count = generated(system, cache, page).planets.len() as u32;
        if slot < first + count {
            return (page, slot - first);
        }
        first += count;
        page += 1;
    }
}

/// An append-only slot mapped through this list's actual assigned stars.
pub fn system_page_of_slot(system: &StarSystem, slot: u32) -> (u32, u32) {
    slot_page(system, &mut Generated::new(), slot)
}

/// Page and orbit index within the page for an append-only list slot.
pub fn page_of_slot(list_id: &str, slot: u32) -> (u32, u32) {
    let (mut page, mut first) = (0, 0);
    loop {
        let capacity = page_capacity(list_id, page);
        if slot < first + capacity {
            return (page, slot - first);
        }
        first += capacity;
        page += 1;
    }
}

/// Places tasks by slot and fills every page they reach with its star's whole
/// planetary system. Deterministic, so repeating it on an already shaped system
/// changes nothing.
pub fn shape(system: &mut StarSystem) {
    let tasks = std::mem::take(&mut system.planets)
        .into_iter()
        .filter_map(|p| p.task)
        .collect();
    place(system, tasks);
}

/// Replaces a system's planets, stars and barycentres with those of the pages
/// the tasks' slots fall on, or of page 0 when there are no tasks.
pub fn place(system: &mut StarSystem, tasks: Vec<Task>) {
    let mut cache = Generated::new();
    let mut pages: BTreeMap<u32, Vec<Option<Task>>> = BTreeMap::new();
    let empty =
        |cache: &mut Generated, page| vec![None; generated(system, cache, page).planets.len()];
    for task in tasks {
        let (page, index) = slot_page(system, &mut cache, task.slot);
        pages.entry(page).or_insert_with(|| empty(&mut cache, page))[index as usize] = Some(task);
    }
    // A reserved page is still a real star and has its whole planetary system,
    // even when its last task disappears from the latest snapshot.
    for page in system.page_stars.iter().map(|s| s.page) {
        pages.entry(page).or_insert_with(|| empty(&mut cache, page));
    }
    if pages.is_empty() {
        pages.insert(0, empty(&mut cache, 0));
    }
    let mut planets = Vec::new();
    let mut stars = Vec::new();
    let mut barycentres = Vec::new();
    for (page, tasks) in pages {
        let key = assigned_star(system, page);
        let generated = cache.remove(&page).unwrap_or_else(|| bodies(&key));
        let cell = system.page_star(page).is_some();
        for (hosted, task) in generated.planets.into_iter().zip(tasks) {
            // Exoplanet style: the innermost planet is b.
            let designation = if cell {
                crate::galaxy::cells::planet_designation(&hosted.host, hosted.index)
            } else {
                let name = format!("{} {}", system.name, page + 1);
                designation(&name, "", &hosted.host_letters, hosted.index)
            };
            planets.push(Planet {
                id: hosted.id(),
                designation,
                page,
                orbit_index: hosted.index as u32,
                host: hosted.host,
                orbit: hosted.body.orbit,
                radius_km: hosted.body.radius_km,
                mass_earth: hosted.body.mass_earth,
                class: hosted.body.class.name().into(),
                inertia_factor: Some(hosted.body.structure.inertia_factor()),
                environment: hosted.environment,
                resonance: hosted.body.resonance,
                spin: hosted.body.spin,
                task,
            });
        }
        stars.extend(
            generated
                .stars
                .into_iter()
                .map(|star| space_shared::Star { page, ..star }),
        );
        barycentres.extend(
            generated
                .barycentres
                .into_iter()
                .map(|pair| space_shared::Barycentre { page, ..pair }),
        );
    }
    system.planets = planets;
    system.stars = stars;
    system.barycentres = barycentres;
}

pub fn shape_galaxy(galaxy: &mut Galaxy) {
    galaxy.systems.iter_mut().for_each(shape);
}

#[derive(Debug)]
pub enum PageStarError {
    /// Every visible star in the home cell is taken.
    Exhausted(String),
    /// A recorded assignment names no cell star.
    Invalid(String),
}

impl std::fmt::Display for PageStarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exhausted(list) => write!(f, "no free galactic stars for list {list}"),
            Self::Invalid(star) => write!(f, "invalid assigned star {star}"),
        }
    }
}

impl std::error::Error for PageStarError {}

/// Put each page of a list on a distinct visible star in the home cell, until
/// the pages hold `highest_slot`. `assigned` returns a page's recorded star;
/// `claim` records a new one, refusing stars any page already has. Candidates
/// follow a seeded order per list and page, so picks are repeatable.
pub fn assign_page_stars<E: From<PageStarError>>(
    system: &mut StarSystem,
    highest_slot: Option<u32>,
    sky: &mut crate::galaxy::cells::Sky,
    mut assigned: impl FnMut(u32) -> Result<Option<String>, E>,
    mut claim: impl FnMut(u32, &str) -> Result<bool, E>,
) -> Result<(), E> {
    let home = crate::galaxy::GalaxyModel::milky_way().home_pc;
    let cell = crate::galaxy::cells::Cell::containing(10, home).expect("galactic home");
    let count = sky.tier_counts(cell)[cell.level()];
    let reach = crate::galaxy::population::reach_sq_per_luminosity(
        crate::galaxy::population::VISIBLE_MAGNITUDE,
    );
    let offset = |position_pc: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|axis| position_pc[axis] - home[axis])
    };
    system.page_stars.clear();
    let mut capacity = 0;
    let mut page = 0;
    while page == 0 || highest_slot.is_some_and(|slot| slot >= capacity) {
        let star = if let Some(id) = assigned(page)? {
            sky.star(&id).ok_or(PageStarError::Invalid(id))?
        } else {
            let seed = stable_hash(&format!("{}:{page}:galactic-star", system.id));
            let mut chosen = None;
            for attempt in 0..count {
                let candidate = sky
                    .cell_star_at(cell, seed.wrapping_add(attempt) % count)
                    .expect("ordinal within tier");
                let distance_sq: f64 = offset(candidate.position_pc).iter().map(|d| d * d).sum();
                // Assigned stars must appear in the magnitude-limited sky that
                // the client streams and can pick from the galaxy overview, and
                // have planets to hold tasks: a companion can leave none.
                if distance_sq <= candidate.star.luminosity_solar * reach
                    && planet_count(&candidate.id) > 0
                    && claim(page, &candidate.id)?
                {
                    chosen = Some(candidate);
                    break;
                }
            }
            chosen.ok_or_else(|| PageStarError::Exhausted(system.id.clone()))?
        };
        let position_ly =
            offset(star.position_pc).map(|d| d * crate::galaxy::PC_METRES / crate::LY_METRES);
        capacity += planet_count(&star.id);
        system.page_stars.push(space_shared::PageStar {
            page,
            id: star.id,
            position_ly,
        });
        page += 1;
    }
    system.position_ly = system.page_stars[0].position_ly;
    Ok(())
}

#[cfg(test)]
mod tests;
