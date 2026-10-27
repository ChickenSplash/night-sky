//! Task-free wire systems shared by the client and background body loaders.
use super::{composition::Budget, structure::prototype};
use space_shared::{PageStar, Planet, StarSystem};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Legacy,
    /// Prepare work only. The caller must dispatch these jobs off the frame thread.
    LayeredPrototype,
}

#[derive(Clone)]
pub struct PlanetJob {
    pub id: String,
    pub designation: String,
    pub budget: Budget,
    pub environment: space_shared::PlanetEnvironment,
}

impl PlanetJob {
    pub fn solve(&self) -> Result<prototype::Solved, super::structure::coupled::Error> {
        prototype::solve(&self.budget, &self.environment, Default::default())
    }
}

pub struct Prepared {
    /// Initial legacy presentation, never represented as a solved prototype.
    pub system: StarSystem,
    pub jobs: Vec<PlanetJob>,
    pub initial_generation: Duration,
}

pub fn generated_system(id: &str, position_ly: [f64; 3]) -> Option<StarSystem> {
    generated_system_with_mode(id, position_ly, Mode::Legacy).map(|p| p.system)
}

pub fn generated_system_with_mode(id: &str, position_ly: [f64; 3], mode: Mode) -> Option<Prepared> {
    let start = Instant::now();
    let bodies = crate::galaxy::cells::cell_bodies(id)?;
    let mut jobs = Vec::new();
    let planets = bodies
        .planets
        .into_iter()
        .map(|hosted| {
            if mode == Mode::LayeredPrototype
                && let Some(environment) = &hosted.environment
            {
                jobs.push(PlanetJob {
                    id: hosted.id(),
                    designation: crate::galaxy::cells::planet_designation(
                        &hosted.host,
                        hosted.index,
                    ),
                    budget: hosted.body.budget,
                    environment: environment.clone(),
                });
            }
            Planet {
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
            }
        })
        .collect();
    let system = StarSystem {
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
    };
    Some(Prepared {
        system,
        jobs,
        initial_generation: start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn experimental_mode_prepares_work_without_changing_initial_planets() {
        for id in ["g1e0024924b.2.231", "g13b6ffffcfeba.2.1"] {
            let legacy = generated_system_with_mode(id, [1.0; 3], Mode::Legacy).unwrap();
            let prototype =
                generated_system_with_mode(id, [1.0; 3], Mode::LayeredPrototype).unwrap();
            assert!(legacy.jobs.is_empty());
            assert_eq!(legacy.system, prototype.system);
            assert_eq!(prototype.jobs.len(), prototype.system.planets.len());
            for (job, planet) in prototype.jobs.iter().zip(&prototype.system.planets) {
                assert_eq!(job.id, planet.id);
                assert_eq!(job.designation, planet.designation);
                assert_eq!(job.environment, *planet.environment.as_ref().unwrap());
            }
            eprintln!(
                "Initial generation {id}: {:.3} ms ({} planets)",
                legacy.initial_generation.as_secs_f64() * 1000.0,
                legacy.system.planets.len()
            );
        }
    }
}
