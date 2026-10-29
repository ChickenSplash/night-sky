//! Task-free wire systems shared by the client and background body loaders.
use super::{composition::Budget, structure::prototype};
use space_shared::{PageStar, StarSystem};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Synchronous,
    /// Prepare work only. The caller must dispatch these jobs off the frame thread.
    PrepareRefinement,
}

#[derive(Clone)]
pub struct PlanetJob {
    pub id: String,
    pub designation: String,
    pub budget: Budget,
    pub environment: space_shared::PlanetEnvironment,
    /// Captured at preparation, never stamped from whichever result is live
    /// when this worker eventually completes.
    pub initial_result: space_shared::PlanetResult,
}

impl PlanetJob {
    /// Layered result, or the labelled fallback when the layered solver refuses.
    pub fn resolve(&self) -> prototype::Outcome {
        prototype::resolve(&self.budget, &self.environment, Default::default())
    }
}

pub struct Prepared {
    /// Initial synchronous fallback presentation, never a solved prototype.
    pub system: StarSystem,
    pub jobs: Vec<PlanetJob>,
    pub initial_generation: Duration,
}

pub fn generated_system(id: &str, position_ly: [f64; 3]) -> Option<StarSystem> {
    generated_system_with_mode(id, position_ly, Mode::Synchronous).map(|p| p.system)
}

pub fn generated_system_with_mode(id: &str, position_ly: [f64; 3], mode: Mode) -> Option<Prepared> {
    let start = Instant::now();
    let bodies = crate::galaxy::cells::cell_bodies(id)?;
    let mut jobs = Vec::new();
    let planets = bodies
        .planets
        .into_iter()
        .map(|hosted| {
            if mode == Mode::PrepareRefinement
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
                    initial_result: hosted.body.initial_result.clone(),
                });
            }
            let designation = crate::galaxy::cells::planet_designation(&hosted.host, hosted.index);
            super::physical::export_planet(hosted, 0, designation, None)
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
    fn refinement_preparation_preserves_synchronous_planets() {
        for id in ["g1e0024924b.2.231", "g13b6ffffcfeba.2.1"] {
            let initial = generated_system_with_mode(id, [1.0; 3], Mode::Synchronous).unwrap();
            let prototype =
                generated_system_with_mode(id, [1.0; 3], Mode::PrepareRefinement).unwrap();
            assert!(initial.jobs.is_empty());
            assert_eq!(initial.system, prototype.system);
            assert_eq!(prototype.jobs.len(), prototype.system.planets.len());
            for (job, planet) in prototype.jobs.iter().zip(&prototype.system.planets) {
                assert_eq!(job.id, planet.id);
                assert_eq!(job.designation, planet.designation);
                assert_eq!(job.environment, *planet.environment.as_ref().unwrap());
            }
            eprintln!(
                "Initial generation {id}: {:.3} ms ({} planets)",
                initial.initial_generation.as_secs_f64() * 1000.0,
                initial.system.planets.len()
            );
        }
    }
}
