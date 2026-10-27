//! Natural systems: planetary generation, wire models and body geometry.

pub mod atmosphere;
pub mod clearance;
pub mod composition;
mod demo;
pub mod generated;
mod generation;
pub mod materials;
mod overlay;
pub mod physical;
pub mod shape;
pub mod structure;
pub mod surface;

pub use demo::demo_galaxy;
pub use generated::generated_system;
pub use generation::{
    Body, Class, EARTH_MASS_SOLAR, EARTH_RADIUS_KM, JUPITER_MASS_EARTH, RetentionSolve, bodies,
    galactic_bodies, galactic_planetary_system, planet_count, planetary_system, system_age_gyr,
    system_stars,
};
pub(crate) use generation::{MAX_SYSTEM_APOAPSIS_AU, system_within};
pub use overlay::{
    PageStarError, assign_page_stars, page_capacity, page_of_slot, place, shape, shape_galaxy,
    star_id, system_page_of_slot,
};
pub use shape::{Spheroid, planet_shape, star_shape};
