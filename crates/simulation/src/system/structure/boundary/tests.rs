use super::*;
use crate::system::atmosphere::{Gas, ONE_BAR_PA};
use crate::system::composition::Inventory;
use crate::system::{clearance::Policy, physical};
use space_shared::AtmosphereReference;

fn world(mass: f64, light: f64, water: f64, nitrogen: f64, carbon: f64) -> Budget {
    let source = Inventory::from_mass_fractions(
        mass * EARTH_MASS_KG,
        &[
            (
                Material::Metal,
                (1.0 - light - water - nitrogen - carbon) * 0.5,
            ),
            (
                Material::Silicate,
                (1.0 - light - water - nitrogen - carbon) * 0.5,
            ),
            (Material::Hydrogen, light * 0.75),
            (Material::Helium, light * 0.25),
            (Material::Water, water),
            (Material::Nitrogen, nitrogen),
            (Material::CarbonDioxide, carbon),
        ],
    )
    .unwrap();
    let inventory = |materials: &[Material]| {
        Inventory::new(
            &materials
                .iter()
                .map(|&m| (m, source.mass_kg(m)))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    Budget::new(
        source,
        inventory(&[Material::Metal, Material::Silicate]),
        inventory(&[Material::Water, Material::Nitrogen, Material::CarbonDioxide]),
        inventory(&[Material::Hydrogen, Material::Helium]),
    )
    .unwrap()
}

#[test]
fn motivating_catalogue_planet_has_inventory_backed_boundary_not_one_bar() {
    let system = crate::system::generated_system("g13b6ffffcfeba.2.1", [0.0; 3]).unwrap();
    let planet = system
        .planets
        .iter()
        .find(|p| p.id == "g13b6ffffcfeba.2.1/1")
        .unwrap();
    let data = physical::validate(planet).unwrap().unwrap();
    let air = planet
        .environment
        .as_ref()
        .unwrap()
        .atmosphere
        .as_ref()
        .unwrap();
    assert_eq!(air.reference_level, AtmosphereReference::Surface);
    assert!(
        data.approximations
            .iter()
            .any(|a| a == "thin_boundary_column")
    );
    assert!(air.pressure_pa > 100.0 * ONE_BAR_PA);
    assert!(air.pressure_pa < 2000.0 * ONE_BAR_PA);
    assert_eq!(data.reference_radius_m, data.condensed_radius_m);
    let gas_mass: f64 = data.materials.iter().map(|p| p.gas_kg).sum();
    let expected =
        gas_mass * data.reference_gravity_m_s2 / (4.0 * PI * data.reference_radius_m.powi(2));
    assert!((air.pressure_pa / expected - 1.0).abs() < 1e-12);
    let physical_air = GasAtmosphere::try_from(air).unwrap();
    let gm = GRAVITATIONAL_CONSTANT * data.retained_mass_kg;
    let one_bar = physical_air
        .pressure_level_altitude_m(ONE_BAR_PA, gm, data.reference_radius_m)
        .unwrap()
        .unwrap();
    assert!(one_bar > 0.0 && one_bar < air.extent_m);
    let optics = physical_air.optics(gm, data.reference_radius_m).unwrap();
    assert!(
        optics
            .rayleigh_m_inv
            .iter()
            .all(|c| c.is_finite() && *c > 0.0)
    );
    let clearance = Policy::default().planet(planet).unwrap();
    assert!(clearance.preferred_m > clearance.exclusion_m);
    eprintln!(
        "boundary regression: pressure={:.4} bar radius={:.3} km one_bar={:.3} km extent={:.3} km gas={:.6e} kg",
        air.pressure_pa / ONE_BAR_PA,
        data.reference_radius_m / 1e3,
        one_bar / 1e3,
        air.extent_m / 1e3,
        gas_mass
    );
    // A pressure-only label substitution must fail validating transport.
    let mut corrupt = planet.clone();
    corrupt
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .pressure_pa = ONE_BAR_PA;
    assert!(physical::validate(&corrupt).is_err());
}

#[test]
fn catalogue_giant_keeps_an_unresolved_one_bar_reference() {
    // Checkpoint 6 comparison example for the motivating boundary world.
    let system = crate::system::generated_system("g13b6ffffdda21.2.0", [0.0; 3]).unwrap();
    let planet = system
        .planets
        .iter()
        .find(|p| p.id == "g13b6ffffdda21.2.0/6")
        .unwrap();
    let data = physical::validate(planet).unwrap().unwrap();
    let air = planet
        .environment
        .as_ref()
        .unwrap()
        .atmosphere
        .as_ref()
        .unwrap();
    assert_eq!(planet.class, "Gas giant");
    assert_eq!(air.reference_level, AtmosphereReference::OneBar);
    assert_eq!(air.pressure_pa, ONE_BAR_PA);
    assert!(
        data.approximations
            .iter()
            .any(|a| a == "deep_boundary_unresolved")
    );
    assert!(data.reference_radius_m > data.condensed_radius_m);
}

#[test]
fn supported_boundaries_cover_airless_trace_and_thick_secondary_columns() {
    for light in [0.0, 1e-15] {
        let (s, b, air) = resolve(&world(1.0, light, 0.0, 0.0, 0.0), 1361.0, 4.5, 280.0).unwrap();
        assert_eq!(s.reference(), RadiusReference::SolidSurface);
        assert_eq!(s.radius_m(), s.condensed_radius_m());
        assert!(air.is_none());
        assert_eq!(b.retained().total_mass_kg(), s.mass_kg());
    }
    let mut pressures = Vec::new();
    for nitrogen in [1e-5, 1e-4, 1e-3, 9e-3] {
        let budget = world(19.0, 0.0, 0.0, nitrogen, 0.0);
        let (s, b, air) = resolve(&budget, 1361.0, 4.5, 300.0).unwrap();
        assert_eq!(s.reference(), RadiusReference::SolidSurface);
        assert!(
            s.approximations()
                .contains(&Approximation::ThinBoundaryColumn)
        );
        let air = air.unwrap();
        assert_eq!(air.reference_level(), ReferenceLevel::Surface);
        assert_eq!(air.composition().fraction(Gas::Nitrogen), 1.0);
        pressures.push(air.pressure_pa());
        for material in Material::ALL {
            assert_eq!(
                budget.retained().mass_kg(material),
                b.retained().mass_kg(material)
            );
        }
    }
    assert!(pressures.windows(2).all(|p| p[1] > p[0]));
    assert!(pressures.last().unwrap() > &(10_000.0 * ONE_BAR_PA));
}

#[test]
fn reference_handoff_preserves_equivalent_one_bar_radius_and_gas_inventory() {
    let at = |water: f64| {
        let b = world(3.0, 1e-5, water, 1e-3, 1e-3);
        resolve(&b, 1361.0, 4.5, 280.0).unwrap()
    };
    let radius_at_one_bar = |s: &Structure, air: GasAtmosphere| {
        s.radius_m()
            + air
                .pressure_level_altitude_m(
                    ONE_BAR_PA,
                    GRAVITATIONAL_CONSTANT * s.mass_kg(),
                    s.radius_m(),
                )
                .unwrap()
                .unwrap()
    };
    for edge in [0.01, 0.02] {
        let (s0, b0, air0) = at(edge * (1.0 - 1e-8));
        let (s1, b1, air1) = at(edge * (1.0 + 1e-8));
        let r0 = radius_at_one_bar(&s0, air0.unwrap());
        let r1 = radius_at_one_bar(&s1, air1.unwrap());
        assert!((r1 / r0 - 1.0).abs() < 1e-7, "{edge}: {r0} {r1}");
        let g0 = b0.reservoir(Reservoir::Gas).total_mass_kg();
        let g1 = b1.reservoir(Reservoir::Gas).total_mass_kg();
        assert!((g1 / g0 - 1.0).abs() < 1e-7);
    }
    assert_ne!(at(0.009).0.reference(), RadiusReference::OneBar);
    assert_eq!(at(0.011).0.reference(), RadiusReference::OneBar);
    assert!(
        at(0.015)
            .0
            .approximations()
            .contains(&Approximation::BoundaryReferenceTransition)
    );
}

#[test]
fn deep_and_water_rich_envelopes_retain_unresolved_one_bar_references() {
    for budget in [
        world(17.0, 0.15, 0.6, 0.0, 0.0),
        world(318.0, 0.9, 0.0, 0.0, 0.0),
    ] {
        let original = derive(&budget, 50.0, 4.5).unwrap();
        let (s, _, air) = resolve(&budget, 50.0, 4.5, 150.0).unwrap();
        assert_eq!(s.reference(), RadiusReference::OneBar);
        assert_eq!(s.radius_m(), original.radius_m());
        assert!(
            s.approximations()
                .contains(&Approximation::DeepBoundaryUnresolved)
        );
        assert_eq!(air.unwrap().pressure_pa(), ONE_BAR_PA);
    }
    for temperature in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(resolve(&world(1.0, 0.0, 0.0, 0.0, 0.0), 1361.0, 4.5, temperature).is_err());
    }
}
