//! Engine-independent simulation. Domain modules own implementations; explicit
//! root exports keep common models and operations convenient for callers.

pub mod background;
pub mod clock;
pub mod craft;
pub mod galaxy;
pub mod light;
pub mod objects;
pub mod orbit;
pub mod search;
pub mod seed;
pub mod star;
pub mod system;
pub mod units;

pub use clock::SimulationClock;
pub use orbit::{
    State, member_position, member_state, member_states, orbit_state, period_seconds,
    planet_period, planet_position, planet_state, position, position_at_eccentric_anomaly,
    star_positions, system_extent_au,
};
pub use seed::{plane_tilt, stable_hash, star_position};
pub use star::{galactic_star, lifetime_gyr, luminosity, spectral_colour, spin_angle, star};
pub use system::{
    Body, Class, EARTH_MASS_SOLAR, EARTH_RADIUS_KM, JUPITER_MASS_EARTH, PageStarError, Spheroid,
    assign_page_stars, bodies, demo_galaxy, galactic_bodies, galactic_planetary_system,
    generated_system, page_capacity, page_of_slot, place, planet_count, planet_shape,
    planetary_system, shape, shape_galaxy, star_id, star_shape, system_age_gyr,
    system_page_of_slot, system_stars,
};
pub use units::{
    AU_METRES, EARTH_SOLAR, LIGHT_SPEED_MS, LY_METRES, SUN_GM, SUN_RADIUS_METRES, YEAR_SECONDS,
};
