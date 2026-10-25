//! Task-free wire systems shared by the client and background body loaders.
use space_shared::{PageStar, Planet, StarSystem};

pub fn generated_system(id: &str, position_ly: [f64; 3]) -> Option<StarSystem> {
    let bodies = crate::galaxy::cells::cell_bodies(id)?;
    let planets = bodies
        .planets
        .into_iter()
        .map(|hosted| Planet {
            id: hosted.id(),
            designation: crate::galaxy::cells::planet_designation(&hosted.host, hosted.index),
            page: 0,
            orbit_index: hosted.index as u32,
            host: hosted.host,
            orbit: hosted.body.orbit,
            radius_km: hosted.body.radius_km,
            mass_earth: hosted.body.mass_earth,
            class: hosted.body.class.name().into(),
            environment: hosted.environment,
            inertia_factor: Some(hosted.body.structure.inertia_factor()),
            physical: Some(super::physical::to_data(&hosted.body)),
            resonance: hosted.body.resonance,
            spin: hosted.body.spin,
            task: None,
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
