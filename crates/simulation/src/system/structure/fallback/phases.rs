//! Water phase layers of a decided surface. The solver's water is one
//! isotherm at the surface temperature, so its phases change at pressures
//! known beforehand. One inward pass from the tabulated surface, through the
//! solver's own water models, finds where each begins and where the water
//! runs out. Nothing is solved for: the radius and mass are already decided.
use super::Kind;
use crate::{
    system::structure::water::{Isotherm, Work},
    units::GRAVITATIONAL_CONSTANT,
};
use std::f64::consts::PI;

/// Steps across the water, or across a phase where that is thinner.
const STEPS: f64 = 8.0;
const MAX_STEPS: usize = 400;
const MAX_EOS_EVALUATIONS: usize = 100_000;

pub(super) struct Layer {
    pub kind: Kind,
    pub mass_kg: f64,
    pub outer_radius_m: f64,
    /// Above the load on the body's surface.
    pub outer_pa: f64,
}

pub(super) struct Phases {
    /// Top of the rock, where the water's mass is used up.
    pub rock_radius_m: f64,
    /// Above the load on the body's surface.
    pub rock_pa: f64,
    /// Innermost first.
    pub layers: Vec<Layer>,
}

/// None where the water models have no answer on this isotherm, or the body
/// is all water: the layer then stays one of undecided phase. `estimate_pa`
/// is the table's pressure at the top of the rock, which only sizes the steps.
pub(super) fn walk(
    temperature_k: f64,
    load_pa: f64,
    mass_kg: f64,
    radius_m: f64,
    water_kg: f64,
    estimate_pa: f64,
) -> Option<Phases> {
    let rock_kg = mass_kg - water_kg;
    if !(rock_kg > 0.0 && water_kg > 0.0 && radius_m > 0.0 && estimate_pa > 0.0) {
        return None;
    }
    let isotherm = Isotherm::new(temperature_k, load_pa).ok()?;
    let mut work = Work::default();
    // Radius and enclosed mass against pressure, which rises inwards.
    let mut slope = |pressure: f64, [radius, mass]: [f64; 2], branch: usize| {
        let density = isotherm
            .density_kg_m3(pressure, branch, &mut work, MAX_EOS_EVALUATIONS)
            .ok()?;
        (density > 0.0 && radius > 0.0 && mass > 0.0).then(|| {
            [
                -radius * radius / (GRAVITATIONAL_CONSTANT * mass * density),
                -4.0 * PI * radius.powi(4) / (GRAVITATIONAL_CONSTANT * mass),
            ]
        })
    };
    let mut step = |pressure: f64, y: [f64; 2], h: f64, branch: usize| {
        let at = |k: [f64; 2], f: f64| [y[0] + f * h * k[0], y[1] + f * h * k[1]];
        let k1 = slope(pressure, y, branch)?;
        let k2 = slope(pressure + 0.5 * h, at(k1, 0.5), branch)?;
        let k3 = slope(pressure + 0.5 * h, at(k2, 0.5), branch)?;
        let k4 = slope(pressure + h, at(k3, 1.0), branch)?;
        let next = [0, 1].map(|i| y[i] + h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]));
        next.iter()
            .all(|v| v.is_finite() && *v > 0.0)
            .then_some(next)
    };

    let mut branch = isotherm.branch_at(load_pa);
    let (mut pressure, mut y) = (load_pa, [radius_m, mass_kg]);
    // Outermost first: branch, mass, outer radius, outer pressure.
    let mut found: Vec<(usize, f64, f64, f64)> = Vec::new();
    let mut outer = (pressure, y);
    let mut steps = 0;
    let (rock_radius_m, rock_pressure) = 'water: loop {
        let cut = match branch {
            0 => f64::INFINITY,
            _ => isotherm.boundaries()[branch - 1],
        };
        let span = cut.min(load_pa + estimate_pa) - pressure;
        let h = if span > 0.0 { span } else { estimate_pa } / STEPS;
        while pressure < cut {
            steps += 1;
            if steps > MAX_STEPS {
                return None;
            }
            let dp = h.min(cut - pressure);
            let next = step(pressure, y, dp, branch)?;
            if next[1] <= rock_kg {
                // Mass is nearly linear in pressure across one step.
                let mut fraction = (y[1] - rock_kg) / (y[1] - next[1]);
                let mut end = next;
                for _ in 0..3 {
                    end = step(pressure, y, fraction * dp, branch)?;
                    fraction *= (y[1] - rock_kg) / (y[1] - end[1]);
                    if !(fraction > 0.0 && fraction <= 1.0) {
                        return None;
                    }
                }
                found.push((branch, outer.1[1] - rock_kg, outer.1[0], outer.0));
                break 'water (end[0], pressure + fraction * dp);
            }
            y = next;
            pressure = if dp == cut - pressure {
                cut
            } else {
                pressure + dp
            };
        }
        found.push((branch, outer.1[1] - y[1], outer.1[0], outer.0));
        outer = (pressure, y);
        branch -= 1;
    };

    // The solver changes model inside a phase too. One layer a phase here.
    let mut layers: Vec<Layer> = Vec::new();
    for (branch, mass, outer_radius_m, outer_pressure) in found.into_iter().rev() {
        if mass.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            continue;
        }
        let kind = Kind::of(isotherm.phase(branch));
        match layers.last_mut() {
            Some(last) if last.kind == kind => {
                last.mass_kg += mass;
                last.outer_radius_m = outer_radius_m;
                last.outer_pa = outer_pressure - load_pa;
            }
            _ => layers.push(Layer {
                kind,
                mass_kg: mass,
                outer_radius_m,
                outer_pa: outer_pressure - load_pa,
            }),
        }
    }
    // The walk's masses sum to the water up to the last step's closure. The
    // thickest layer takes that remainder, so the water is conserved exactly.
    let others = |skip: usize| -> f64 {
        layers
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != skip)
            .map(|(_, l)| l.mass_kg)
            .sum()
    };
    let largest =
        (0..layers.len()).max_by(|a, b| layers[*a].mass_kg.total_cmp(&layers[*b].mass_kg))?;
    layers[largest].mass_kg = water_kg - others(largest);
    let ordered = layers.windows(2).all(|pair| {
        pair[0].outer_radius_m <= pair[1].outer_radius_m && pair[0].outer_pa >= pair[1].outer_pa
    });
    let rock_pa = rock_pressure - load_pa;
    (layers.iter().all(|l| l.mass_kg > 0.0)
        && ordered
        && rock_radius_m > 0.0
        && rock_radius_m <= layers[0].outer_radius_m
        && rock_pa >= layers[0].outer_pa)
        .then_some(Phases {
            rock_radius_m,
            rock_pa,
            layers,
        })
}
