use super::*;
use crate::system::atmosphere::{Composition, Gas, ReferenceLevel};
use space_shared::{PlanetEnvironment, Spin};

fn earth() -> Planet {
    let gas = GasAtmosphere::new(
        Composition::new(&[(Gas::Nitrogen, 0.79), (Gas::Oxygen, 0.21)]).unwrap(),
        ReferenceLevel::Surface,
        101_325.0,
        288.0,
        None,
    )
    .unwrap();
    Planet {
        mass_earth: 1.0,
        radius_km: 6_371.0,
        class: "rocky".into(),
        spin: Spin {
            period_hours: 24.0,
            ..Default::default()
        },
        environment: Some(PlanetEnvironment {
            atmosphere: Some(gas.to_data(EARTH_SOLAR * SUN_GM, 6.371e6).unwrap()),
            irradiation_w_m2: 1361.0,
            assumed_bond_albedo: 0.3,
            equilibrium_temperature_k: 255.0,
            age_gyr: 4.5,
        }),
        ..Default::default()
    }
}

#[test]
fn generated_clearance_uses_enclosed_reference_gravity_not_exterior_total_mass() {
    let galaxy = crate::demo_galaxy();
    let world = galaxy
        .systems
        .iter()
        .flat_map(|s| &s.planets)
        .find(|p| {
            p.physical
                .as_ref()
                .is_some_and(|d| d.h_he_mass_fraction > 0.3)
        })
        .unwrap();
    let original = Policy::default().planet(world).unwrap();
    let mut lower_gravity = world.clone();
    let result = lower_gravity.initial_result.as_mut().unwrap();
    let radius = result.record.physical.reference_radius_m;
    let enclosed: f64 = result
        .record
        .layers
        .iter()
        .filter(|l| l.outer_radius_m <= radius)
        .map(|l| l.mass_kg)
        .sum();
    let minimum = crate::units::GRAVITATIONAL_CONSTANT * enclosed / radius.powi(2);
    let gravity = (minimum + result.record.physical.reference_gravity_m_s2) * 0.5;
    result.record.physical.reference_gravity_m_s2 = gravity;
    lower_gravity.physical = Some(result.record.physical.clone());
    let data = lower_gravity.environment.as_mut().unwrap();
    let gas = GasAtmosphere::try_from(data.atmosphere.as_ref().unwrap()).unwrap();
    data.atmosphere = Some(gas.to_data(gravity * radius.powi(2), radius).unwrap());
    let bound = Policy::default().planet(&lower_gravity).unwrap();
    assert!(bound.atmosphere_height_m > original.atmosphere_height_m);
    let mut unchecked = lower_gravity;
    unchecked.initial_result = None;
    assert!(Policy::default().planet(&unchecked).is_err());
}

#[test]
fn density_extends_smoothly_beyond_the_optical_cutoff() {
    let p = earth();
    let gas = GasAtmosphere::try_from(p.environment.as_ref().unwrap().atmosphere.as_ref().unwrap())
        .unwrap();
    let c = gas.column(EARTH_SOLAR * SUN_GM, 6.371e6).unwrap();
    assert_eq!(c.density_at(0.0).unwrap(), c.density_kg_m3);
    assert!(
        (c.density_at(c.scale_height_m).unwrap() / c.density_kg_m3
            - (-1.0 / (1.0 + c.scale_height_m / 6.371e6)).exp())
        .abs()
            < 1e-14
    );
    assert!(c.density_at(2.0 * c.extent_m).unwrap() > 0.0);
    assert_eq!(c.density_at(1e300).unwrap(), 0.0);
    assert!(c.density_at(-1.0).is_err());
    assert!(c.density_at(f64::NAN).is_err());
}

#[test]
fn maintenance_free_height_satisfies_the_declared_decay_budget_not_the_optical_cutoff() {
    let p = earth();
    let policy = Policy::default();
    let bound = policy.planet(&p).unwrap();
    let gas = GasAtmosphere::try_from(p.environment.as_ref().unwrap().atmosphere.as_ref().unwrap())
        .unwrap();
    let gm = EARTH_SOLAR * SUN_GM;
    let c = gas.column(gm, 6.371e6).unwrap();
    assert!(bound.atmosphere_height_m > 2.0 * c.extent_m);
    let loss = |h: f64| {
        let r = planet_shape(&p).equatorial + h;
        let vc = (gm / r).sqrt();
        c.density_at(h).unwrap() / policy.ballistic_coefficient_kg_m2
            * (gm * r).sqrt()
            * (1.0 + TAU / (24.0 * 3_600.0) * r / vc).powi(2)
            * policy.duration_s
    };
    assert!(loss(bound.atmosphere_height_m) <= policy.loss_m * (1.0 + 1e-12));
    assert!(loss(bound.atmosphere_height_m - 1.0) > policy.loss_m);
    assert!(bound.atmosphere_height_m < 400_000.0); // Model estimate, not a real Earth thermosphere.
    assert!(
        Policy {
            loss_m: 0.01,
            ..policy
        }
        .planet(&p)
        .unwrap()
        .exclusion_m
            > bound.exclusion_m
    );
    assert!(
        Policy {
            ballistic_coefficient_kg_m2: 1.0,
            ..policy
        }
        .planet(&p)
        .unwrap()
        .exclusion_m
            > bound.exclusion_m
    );
}

#[test]
fn spherical_bound_covers_oblate_equatorial_polar_and_tilted_shells() {
    let policy = Policy::default();
    let shape = Spheroid::of(6e6, 0.2);
    let height = 400_000.0;
    let bound = policy.bound(shape, height, false).unwrap();
    for dir in [
        glam::DVec3::X,
        glam::DVec3::Y,
        glam::DVec3::new(1.0, 1.0, 1.0).normalize(),
    ] {
        assert!(bound.exclusion_m > shape.along(dir).length() + height);
    }
    assert!(bound.exclusion_m > 6e6 + height);
}

#[test]
fn preferred_routes_keep_a_fractional_margin_outside_thick_atmospheres() {
    let policy = Policy::default();
    let shape = Spheroid::of(1e6, 0.0);
    for height in [0.0, 1e5, 1e6, 1e7] {
        let bound = policy.bound(shape, height, height > 1e5).unwrap();
        assert_eq!(
            bound.exclusion_m,
            1e6 + height + policy.hull_radius_m + policy.safety_m
        );
        assert_eq!(bound.preferred_m, KEEP_CLEAR * (1e6 + height));
        assert!(bound.preferred_m - bound.exclusion_m > 0.49 * (1e6 + height));
    }
}

#[test]
fn airless_unknown_invalid_and_extended_envelopes_are_distinguished() {
    let mut p = earth();
    p.environment.as_mut().unwrap().atmosphere = None;
    assert_eq!(
        Policy::default().planet(&p).unwrap().atmosphere_height_m,
        0.0
    );
    p.environment = None;
    assert!(
        Policy::default()
            .planet(&p)
            .unwrap_err()
            .contains("unknown")
    );
    p = earth();
    p.environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .pressure_pa = f64::NAN;
    assert!(Policy::default().planet(&p).is_err());
    let gas = GasAtmosphere::new(
        Composition::new(&[(Gas::Hydrogen, 1.0)]).unwrap(),
        ReferenceLevel::OneBar,
        1e5,
        500.0,
        None,
    )
    .unwrap();
    p.environment.as_mut().unwrap().atmosphere =
        Some(gas.to_data(EARTH_SOLAR * SUN_GM, 6.371e6).unwrap());
    let bound = Policy::default().planet(&p).unwrap();
    assert!(bound.extended_column);
    assert!(
        bound.atmosphere_height_m > gas.column(EARTH_SOLAR * SUN_GM, 6.371e6).unwrap().extent_m
    );
    assert_eq!(bound, Policy::default().planet(&p).unwrap());
}
