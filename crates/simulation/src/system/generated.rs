//! Task-free wire systems shared by the client and background body loaders.
use space_shared::{PageStar, StarSystem};

/// Every planet carries its final result: generation reads its structure
/// from the embedded table, and nothing replaces it later.
pub fn generated_system(id: &str, position_ly: [f64; 3]) -> Option<StarSystem> {
    let bodies = crate::galaxy::cells::cell_bodies(id)?;
    let planets = bodies
        .planets
        .into_iter()
        .map(|hosted| {
            let designation = crate::galaxy::cells::planet_designation(&hosted.host, hosted.index);
            super::physical::export_planet(hosted, 0, designation, None)
        })
        .collect();
    Some(StarSystem {
        id: id.to_owned(),
        name: crate::galaxy::cells::catalogue_name(id),
        position_ly,
        planets,
        stars: bodies.stars,
        barycentres: bodies.barycentres,
        page_stars: vec![PageStar {
            page: 0,
            id: id.to_owned(),
            position_ly,
        }],
    })
}
