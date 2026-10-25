//! Bounded final-orbit H/He survival, using the atmosphere's all-star forcing.
use super::*;
use crate::system::{
    composition::{Budget, Inventory, Material, Reservoir},
    structure::{self, Structure, StructureError},
};
use crate::units::{ATOMIC_MASS_KG, BOLTZMANN_J_K, EARTH_MASS_KG, GRAVITATIONAL_CONSTANT};

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

/// Solve survival = Jeans-inspired survival(final mass, final radius).
/// Bisection needs a sign bracket and continuity, not a monotone radius law.
/// Failure is explicit, never an unreported plausible-looking fallback planet.
pub(super) fn reconcile(
    formation: &Budget,
    flux: f64,
    loss_flux: f64,
    age: f64,
) -> Result<(Budget, Structure, RetentionSolve), StructureError> {
    if !loss_flux.is_finite() || loss_flux < 0.0 {
        return Err(StructureError::InvalidValue("loss flux"));
    }
    let formation = secondary_budget(formation, flux, loss_flux, age)?;
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
    .map_err(|_| StructureError::InvalidValue("source gas"))?;
    if gas.total_mass_kg() == 0.0 {
        return Ok((
            formation,
            structure::derive(&formation, flux, age)?,
            RetentionSolve::NoEnvelope,
        ));
    }
    let at = |survival: f64| -> Result<(Budget, Structure), StructureError> {
        let gas = gas
            .scaled(survival)
            .map_err(|_| StructureError::InvalidValue("retained gas"))?;
        let budget = Budget::new(
            *formation.source(),
            *formation.reservoir(Reservoir::Interior),
            *formation.reservoir(Reservoir::CondensedVolatiles),
            gas,
        )
        .map_err(|_| StructureError::InvalidValue("retained budget"))?;
        let physical = structure::derive(&budget, flux, age)?;
        Ok((budget, physical))
    };
    let (mut low, mut high) = (0.0, 1.0);
    for steps in 1..=MAX_STEPS {
        let survival = (low + high) * 0.5;
        let (budget, physical) = at(survival)?;
        let target = hydrogen_survival(&physical, flux, loss_flux);
        if (survival - target).abs() <= SURVIVAL_TOLERANCE || high - low <= SURVIVAL_TOLERANCE {
            return Ok((budget, physical, RetentionSolve::Converged { steps }));
        }
        if survival > target {
            high = survival;
        } else {
            low = survival;
        }
    }
    Err(StructureError::OutsideDomain("retention did not converge"))
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

fn secondary_budget(
    previous: &Budget,
    flux: f64,
    loss_flux: f64,
    age: f64,
) -> Result<Budget, StructureError> {
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
    .map_err(|_| StructureError::InvalidValue("source interior"))?;
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
    .map_err(|_| StructureError::InvalidValue("source volatiles"))?;
    let gas = Inventory::new(&[
        (Material::Hydrogen, source.mass_kg(Material::Hydrogen)),
        (Material::Helium, source.mass_kg(Material::Helium)),
    ])
    .map_err(|_| StructureError::InvalidValue("source envelope"))?;
    let initial = Budget::new(*source, interior, condensed, gas)
        .map_err(|_| StructureError::InvalidValue("source budget"))?;
    // Fixed pre-loss structure at the final irradiation/age. This deliberately
    // avoids claiming a coupled multi-species evolution/escape solution.
    let physical = structure::derive(&initial, flux, age)?;
    let escape = (2.0 * GRAVITATIONAL_CONSTANT * physical.mass_kg() / physical.radius_m()).sqrt();
    let survival = crate::system::atmosphere::generation::secondary_retention(escape, loss_flux);
    let condensed = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            condensed.mass_kg(m) * (1.0 - accessible_fraction(m) * (1.0 - survival)),
        )
    }))
    .map_err(|_| StructureError::InvalidValue("retained volatiles"))?;
    Budget::new(*source, interior, condensed, gas)
        .map_err(|_| StructureError::InvalidValue("secondary escape budget"))
}

fn hydrogen_survival(physical: &Structure, flux: f64, loss_flux: f64) -> f64 {
    let gm = GRAVITATIONAL_CONSTANT * physical.mass_kg();
    // Fixed albedo estimate here keeps retention independent of atmosphere RNG.
    let equilibrium = (flux * 0.7 / (4.0 * 5.670_374_419e-8)).powf(0.25).max(20.0);
    let exosphere = (1000.0 * (loss_flux / 1361.0).max(0.01).powf(0.2)).max(2.0 * equilibrium);
    let jeans = gm / physical.radius_m()
        * super::super::atmosphere::Gas::Hydrogen.molecular_mass_u()
        * ATOMIC_MASS_KG
        / (BOLTZMANN_J_K * exosphere);
    1.0 / (1.0 + (25.0 / jeans).powi(4))
}

/// Called once after the complete hierarchy and final orbits exist. The caller
/// caches stellar phase samples and shares the heating with atmosphere generation.
pub(crate) fn finalise(
    body: &mut Body,
    id: &str,
    host_mass: f64,
    flux: f64,
    loss_flux: f64,
    age: f64,
) {
    let (budget, structure, solve) = reconcile(&body.budget, flux, loss_flux, age)
        .expect("generated inventory must have supported convergent retained structure");
    body.mass_earth = structure.mass_kg() / EARTH_MASS_KG;
    body.radius_km = structure.radius_m() / 1000.0;
    body.class = materials::classify(&budget, &structure);
    body.budget = budget;
    body.structure = structure;
    body.retention_solve = solve;
    refresh_spin(body, id, host_mass);
}

pub(crate) fn refresh_spin(body: &mut Body, id: &str, host_mass: f64) {
    body.spin = planet_spin(&mut Rng::new(&format!("{id}:spin:v2")), body, host_mass);
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
            let (budget, structure, status) = reconcile(&initial, flux, flux, 4.5).unwrap();
            let RetentionSolve::Converged { steps } = status else {
                panic!("expected envelope solve")
            };
            assert!(steps <= MAX_STEPS);
            let survival = budget.reservoir(Reservoir::Gas).total_mass_kg()
                / initial.reservoir(Reservoir::Gas).total_mass_kg();
            assert!(
                (survival - hydrogen_survival(&structure, flux, flux)).abs()
                    < 3.0 * SURVIVAL_TOLERANCE
            );
            assert!(survival <= previous);
            previous = survival;
            for m in Material::ALL {
                let source = budget.source().mass_kg(m);
                let accounted = budget.retained().mass_kg(m) + budget.escaped().mass_kg(m);
                assert!((source - accounted).abs() <= source * 1e-14);
            }
            let repeated = reconcile(&budget, flux, flux, 4.5).unwrap();
            assert_eq!((budget, structure, status), repeated);
        }
        assert!(reconcile(&initial, 1361.0, f64::NAN, 4.5).is_err());
        assert!(reconcile(&initial, -1.0, 1361.0, 4.5).is_err());
        assert!(reconcile(&initial, 1361.0, 1361.0, 0.0).is_err());
    }

    #[test]
    fn formation_domain_grid_converges_without_fallback_or_compounded_escape() {
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
                        let (budget, physical, status) =
                            reconcile(&formation, flux, loss, age).unwrap();
                        assert_eq!(
                            reconcile(&budget, flux, loss, age).unwrap(),
                            (budget, physical.clone(), status)
                        );
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
                                    (survival - hydrogen_survival(&physical, flux, loss)).abs()
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
}
