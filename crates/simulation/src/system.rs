//! Natural systems: planetary generation, wire models and body geometry.

mod generated;
mod generation;
pub mod shape;
pub mod surface;

pub use generated::generated_system;
pub use generation::{
    Body, Class, EARTH_MASS_SOLAR, EARTH_RADIUS_KM, JUPITER_MASS_EARTH, PageStarError,
    assign_page_stars, bodies, demo_galaxy, galactic_bodies, galactic_planetary_system,
    page_capacity, page_of_slot, place, planet_count, planetary_system, shape, shape_galaxy,
    star_id, system_age_gyr, system_page_of_slot, system_stars,
};
pub(crate) use generation::{MAX_SYSTEM_APOAPSIS_AU, system_within};
pub use shape::{Spheroid, planet_shape, star_shape};
