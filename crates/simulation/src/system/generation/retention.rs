//! Bounded final-orbit survival on fast fallback facts and immutable forcing,
//! then the one synchronous surface decision (`structure::fallback::decide`).
use super::*;
use crate::system::{
    composition::{Budget, Inventory, Material, Reservoir},
    forcing::Forcing,
    structure::{self, fallback::Fallback},
};
use crate::units::{ATOMIC_MASS_KG, BOLTZMANN_J_K, EARTH_MASS_KG, GRAVITATIONAL_CONSTANT};
use space_shared::PlanetEnvironment;

pub const MAX_STEPS: u8 = 40;
pub const SURVIVAL_TOLERANCE: f64 = 1e-8;
/// Tunable exposure/outgassing priors, not measured partition coefficients.
pub const ACCESSIBLE_WATER_FRACTION: f64 = 1e-5;
pub const ACCESSIBLE_SECONDARY_FRACTION: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionSolve {
    NoEnvelope,
    Converged { steps: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetentionError {
    InvalidInventory(&'static str),
    NonConvergence,
}

impl std::fmt::Display for RetentionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInventory(name) => write!(f, "invalid retention {name}"),
            Self::NonConvergence => write!(f, "retention did not converge"),
        }
    }
}

impl std::error::Error for RetentionError {}

/// Solve survival = Jeans-inspired survival(final mass, final radius).
/// Bisection needs a sign bracket and continuity, not a monotone radius law.
/// Trial structure is explicitly prescribed fallback, never a layered solve.
/// Failure to converge remains an error, not a silently accepted trial.
pub(super) fn reconcile(
    formation: &Budget,
    forcing: &Forcing,
) -> Result<(Budget, Fallback, RetentionSolve), RetentionError> {
    let environment = forcing.environment();
    let formation = secondary_budget(formation, forcing)?;
    // Always solve against the immutable source, not the previously retained
    // envelope. Re-evaluation must not apply the loss history a second time.
    let gas = Inventory::new(&[
        (
            Material::Hydrogen,
            formation.source().mass_kg(Material::Hydrogen),
        ),
        (
            Material::Helium,
            formation.source().mass_kg(Material::Helium),
        ),
    ])
    .map_err(|_| RetentionError::InvalidInventory("source gas"))?;
    if gas.total_mass_kg() == 0.0 {
        return Ok((
            formation,
            structure::fallback::initial(&formation, &environment),
            RetentionSolve::NoEnvelope,
        ));
    }
    let at = |survival: f64| -> Result<(Budget, Fallback), RetentionError> {
        let gas = gas
            .scaled(survival)
            .map_err(|_| RetentionError::InvalidInventory("retained gas"))?;
        let budget = Budget::new(
            *formation.source(),
            *formation.reservoir(Reservoir::Interior),
            *formation.reservoir(Reservoir::CondensedVolatiles),
            gas,
        )
        .map_err(|_| RetentionError::InvalidInventory("retained budget"))?;
        let physical = structure::fallback::initial(&budget, &environment);
        Ok((budget, physical))
    };
    let (mut low, mut high) = (0.0, 1.0);
    for steps in 1..=MAX_STEPS {
        let survival = (low + high) * 0.5;
        let (budget, physical) = at(survival)?;
        let target = hydrogen_survival(&physical, forcing);
        if (survival - target).abs() <= SURVIVAL_TOLERANCE || high - low <= SURVIVAL_TOLERANCE {
            return Ok((budget, physical, RetentionSolve::Converged { steps }));
        }
        if survival > target {
            high = survival;
        } else {
            low = survival;
        }
    }
    Err(RetentionError::NonConvergence)
}

/// Accessible volatile supply is a population prescription, not solved outgassing.
/// Protected material remains condensed; only accessible material can escape.
pub(crate) fn accessible_fraction(material: Material) -> f64 {
    match material {
        Material::Water => ACCESSIBLE_WATER_FRACTION,
        Material::Metal | Material::Silicate | Material::Hydrogen | Material::Helium => 0.0,
        _ => ACCESSIBLE_SECONDARY_FRACTION,
    }
}

fn secondary_budget(previous: &Budget, forcing: &Forcing) -> Result<Budget, RetentionError> {
    let source = previous.source();
    let interior = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            if m.properties().gas.is_none() {
                source.mass_kg(m)
            } else {
                0.0
            },
        )
    }))
    .map_err(|_| RetentionError::InvalidInventory("source interior"))?;
    let condensed = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            if accessible_fraction(m) > 0.0 {
                source.mass_kg(m)
            } else {
                0.0
            },
        )
    }))
    .map_err(|_| RetentionError::InvalidInventory("source volatiles"))?;
    let gas = Inventory::new(&[
        (Material::Hydrogen, source.mass_kg(Material::Hydrogen)),
        (Material::Helium, source.mass_kg(Material::Helium)),
    ])
    .map_err(|_| RetentionError::InvalidInventory("source envelope"))?;
    let initial = Budget::new(*source, interior, condensed, gas)
        .map_err(|_| RetentionError::InvalidInventory("source budget"))?;
    // Fixed pre-loss structure at the final irradiation/age. This deliberately
    // avoids claiming a coupled multi-species evolution/escape solution.
    let physical = structure::fallback::initial(&initial, &forcing.environment());
    let escape = (2.0 * GRAVITATIONAL_CONSTANT * physical.mass_kg() / physical.radius_m).sqrt();
    let survival = secondary_retention(escape, forcing.loss_flux_w_m2);
    let condensed = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            condensed.mass_kg(m) * (1.0 - accessible_fraction(m) * (1.0 - survival)),
        )
    }))
    .map_err(|_| RetentionError::InvalidInventory("retained volatiles"))?;
    Budget::new(*source, interior, condensed, gas)
        .map_err(|_| RetentionError::InvalidInventory("secondary escape budget"))
}

fn hydrogen_survival(physical: &Fallback, forcing: &Forcing) -> f64 {
    let gm = GRAVITATIONAL_CONSTANT * physical.mass_kg();
    // Fixed albedo estimate here keeps retention independent of atmosphere RNG.
    let equilibrium = (forcing.irradiation_w_m2 * 0.7 / (4.0 * 5.670_374_419e-8))
        .powf(0.25)
        .max(20.0);
    let exosphere =
        (1000.0 * (forcing.loss_flux_w_m2 / 1361.0).max(0.01).powf(0.2)).max(2.0 * equilibrium);
    let jeans = gm / physical.radius_m
        * super::super::atmosphere::Gas::Hydrogen.molecular_mass_u()
        * ATOMIC_MASS_KG
        / (BOLTZMANN_J_K * exosphere);
    1.0 / (1.0 + (25.0 / jeans).powi(4))
}

/// Empirical shoreline-inspired trend, not a universal survival boundary.
/// At Earth's escape speed, the nominal transition is ten present solar fluxes.
pub(crate) fn secondary_retention(escape_m_s: f64, loss_flux: f64) -> f64 {
    let threshold = 10.0 * (escape_m_s / 11_200.0).powi(4);
    1.0 / (1.0 + (loss_flux / 1361.0 / threshold).powf(1.5))
}

/// Called once after the complete hierarchy and final orbits exist. The caller
/// caches stellar phase samples. Mass, radius, inertia, reservoirs, atmosphere
/// and spin all come from one partitioned result and are installed together.
pub(crate) fn finalise(
    body: &mut Body,
    id: &str,
    host_mass: f64,
    forcing: &Forcing,
) -> PlanetEnvironment {
    let (budget, fallback, solve) = reconcile(&body.budget, forcing)
        .expect("generated inventory must have supported convergent retained structure");
    let prepared = crate::system::physical::initial_with_atmosphere(
        &fallback,
        &forcing.environment(),
        forcing.revision(id, &budget),
    )
    .unwrap_or_else(|error| panic!("{id}: retained fallback: {error}"));
    let environment = prepared.live_environment();
    let facts = &prepared.result.record.physical;
    body.mass_earth = facts.retained_mass_kg / EARTH_MASS_KG;
    body.radius_km = facts.reference_radius_m / 1000.0;
    body.class = materials::classify(facts);
    body.budget = prepared.budget;
    body.initial_result = prepared.result;
    body.retention_solve = solve;
    body.decision = prepared.decision;
    body.blowoff = prepared.blowoff;
    body.spin = planet_spin(&mut Rng::new(&format!("{id}:spin:v2")), body, host_mass);
    environment
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formation() -> Budget {
        let interior = Inventory::new(&[(Material::Silicate, 5.0 * EARTH_MASS_KG)]).unwrap();
        let gas = Inventory::new(&[
            (Material::Hydrogen, 0.75 * EARTH_MASS_KG),
            (Material::Helium, 0.25 * EARTH_MASS_KG),
        ])
        .unwrap();
        Budget::new(
            interior.checked_add(&gas).unwrap(),
            interior,
            Inventory::default(),
            gas,
        )
        .unwrap()
    }

    #[test]
    fn bounded_solve_conserves_species_and_reproduces_the_survival_equation() {
        let initial = formation();
        let mut previous = 1.0;
        for flux in [0.0, 13.61, 1361.0, 1e5, 1e8] {
            let forcing = Forcing::new(flux, flux, 0.3, 4.5).unwrap();
            let (budget, structure, status) = reconcile(&initial, &forcing).unwrap();
            assert_eq!(structure.reason, None);
            crate::system::physical::validate_record(&crate::system::physical::fallback(
                &structure,
            ))
            .unwrap();
            let RetentionSolve::Converged { steps } = status else {
                panic!("expected envelope solve")
            };
            assert!(steps <= MAX_STEPS);
            let survival = budget.reservoir(Reservoir::Gas).total_mass_kg()
                / initial.reservoir(Reservoir::Gas).total_mass_kg();
            assert!(
                (survival - hydrogen_survival(&structure, &forcing)).abs()
                    < 3.0 * SURVIVAL_TOLERANCE
            );
            assert!(survival <= previous);
            previous = survival;
            for m in Material::ALL {
                let source = budget.source().mass_kg(m);
                let accounted = budget.retained().mass_kg(m) + budget.escaped().mass_kg(m);
                assert!((source - accounted).abs() <= source * 1e-14);
            }
            let repeated = reconcile(&budget, &forcing).unwrap();
            assert_eq!((budget, structure, status), repeated);
        }
    }

    #[test]
    fn fallback_trial_grid_converges_without_compounded_escape() {
        let mut solved = 0;
        let mut absent = 0;
        for host in [0.08, 0.2, 1.0, 3.0, 20.0] {
            for location in [0.01, 0.1, 1.0, 10.0, 100.0] {
                for seed in 0..8 {
                    let formation = materials::draw(
                        &mut Rng::new(&format!("retention-grid:{seed}")),
                        location * 2.7,
                        2.7,
                        host,
                        false,
                    );
                    for (flux, loss, age) in [
                        (0.0, 0.0, 0.001),
                        (1.0, 10.0, 0.1),
                        (1361.0, 13610.0, 4.5),
                        (1e8, 1e10, 13.0),
                    ] {
                        let forcing = Forcing::new(flux, loss, 0.3, age).unwrap();
                        let (budget, physical, status) = reconcile(&formation, &forcing).unwrap();
                        crate::system::physical::validate_record(
                            &crate::system::physical::fallback(&physical),
                        )
                        .unwrap();
                        assert_eq!(
                            reconcile(&budget, &forcing).unwrap(),
                            (budget, physical.clone(), status)
                        );
                        // The surface decision is total on every retained budget,
                        // and what it strips is still accounted as escape.
                        let prepared = crate::system::physical::initial_with_atmosphere(
                            &physical,
                            &forcing.environment(),
                            forcing.revision("grid", &budget),
                        )
                        .unwrap();
                        crate::system::physical::validate_result(&prepared.result).unwrap();
                        for m in Material::ALL {
                            let decided = &prepared.budget;
                            assert_eq!(decided.source().mass_kg(m), budget.source().mass_kg(m));
                            assert!(decided.retained().mass_kg(m) <= budget.retained().mass_kg(m));
                            assert!(
                                decided.escaped().mass_kg(m)
                                    >= budget.escaped().mass_kg(m) + prepared.blowoff.mass_kg(m)
                                        - 1e-9 * budget.source().mass_kg(m)
                            );
                        }
                        match status {
                            RetentionSolve::NoEnvelope => absent += 1,
                            RetentionSolve::Converged { steps } => {
                                solved += 1;
                                assert!(steps <= MAX_STEPS);
                                let light = |i: &Inventory| {
                                    i.mass_kg(Material::Hydrogen) + i.mass_kg(Material::Helium)
                                };
                                let survival = light(budget.retained()) / light(budget.source());
                                assert!(
                                    (survival - hydrogen_survival(&physical, &forcing)).abs()
                                        < 3.0 * SURVIVAL_TOLERANCE
                                );
                            }
                        }
                        for m in Material::ALL {
                            let source = budget.source().mass_kg(m);
                            let accounted =
                                budget.retained().mass_kg(m) + budget.escaped().mass_kg(m);
                            assert!((source - accounted).abs() <= source * 1e-14);
                        }
                    }
                }
            }
        }
        assert!(solved > 0 && absent > 0);
    }

    #[test]
    fn retention_needs_neither_legacy_domain_nor_preexisting_atmosphere_partition() {
        // Below the legacy total-mass domain, with no meaningful previous split.
        let inventory = Inventory::new(&[
            (Material::Silicate, 1e-6 * EARTH_MASS_KG),
            (Material::Water, 1e-8 * EARTH_MASS_KG),
            (Material::Nitrogen, 1e-10 * EARTH_MASS_KG),
        ])
        .unwrap();
        let formation = Budget::new(
            inventory,
            inventory,
            Inventory::default(),
            Inventory::default(),
        )
        .unwrap();
        let forcing = Forcing::new(1361.0, 1e6, 0.4, 4.5).unwrap();
        let (retained, fallback, status) = reconcile(&formation, &forcing).unwrap();
        assert_eq!(status, RetentionSolve::NoEnvelope);
        assert_eq!(retained.source(), &inventory);
        assert!(retained.escaped().mass_kg(Material::Nitrogen) > 0.0);
        crate::system::physical::validate_record(&crate::system::physical::fallback(&fallback))
            .unwrap();
        let repartitioned = Budget::new(
            *retained.source(),
            *retained.retained(),
            Inventory::default(),
            Inventory::default(),
        )
        .unwrap();
        assert_eq!(
            reconcile(&repartitioned, &forcing).unwrap(),
            (retained, fallback, status)
        );
    }
}
