//! Offline preparation of the structure table: every cell is one run of the
//! reference solver. Run through `scripts/prepare-structure-table.sh`.
use super::*;
use crate::{
    system::{
        composition::{Inventory, Material},
        materials::water as eos,
        structure::{coupled, dry, dry::Sample, envelope, water},
    },
    units::EARTH_MASS_KG,
};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;

// The retired closed-form envelope radius, kept only as the surrogate for
// cells the solver refuses. Lopez and Fortney (2014), equation 4, for the
// thickness; a tunable self-compressed giant, not a published fit, above it.
const EARTH_REFERENCE_RADIUS_M: f64 = 6_371_000.0;
const EARTH_REFERENCE_FLUX_W_M2: f64 = 1361.0;
const ENVELOPE_THICKNESS_EARTH: f64 = 2.06;
const ENVELOPE_MASS_EXPONENT: f64 = -0.21;
const ENVELOPE_FRACTION_EXPONENT: f64 = 0.59;
const ENVELOPE_FLUX_EXPONENT: f64 = 0.044;
const ENVELOPE_AGE_EXPONENT: f64 = -0.18;
const ENVELOPE_FIT_MASS_RANGE: [f64; 2] = [1.0, 20.0];
const ENVELOPE_FIT_FLUX_RANGE: [f64; 2] = [0.1, 1000.0];
const ENVELOPE_FIT_AGE_RANGE: [f64; 2] = [0.1, 10.0];
const ENVELOPE_FIT_MAX_FRACTION: f64 = 0.2;
const GIANT_RADIUS_EARTH: f64 = 11.2;
const GIANT_REFERENCE_MASS_EARTH: f64 = 318.0;
const GIANT_TURNOVER_MASS_EARTH: f64 = 95.0;
const GIANT_HIGH_MASS_EXPONENT: f64 = -0.04;
const GIANT_BLEND_FRACTION_RANGE: [f64; 2] = [0.1, 0.5];

pub(super) fn parallel<T: Send>(count: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    // Half the machine unless told otherwise: leave the rest for whoever is
    // using it.
    let threads = std::env::var("NS_TABLE_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()) / 2)
        .max(1);
    let next = AtomicUsize::new(0);
    let mut results: Vec<(usize, T)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= count {
                            break done;
                        }
                        done.push((index, work(index)));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, value)| value).collect()
}

/// Load a water surface is solved under: nothing over ice, and just above
/// saturation over liquid, as a saturated column leaves it.
pub(super) fn water_load_pa(temperature_k: f64) -> f64 {
    if temperature_k >= 273.16 {
        1.05 * eos::saturation_pressure_pa(temperature_k).unwrap_or(f64::INFINITY)
    } else {
        0.0
    }
}

/// What the table keeps of one solid body.
#[derive(Clone, Copy, Debug)]
pub(super) struct Body {
    pub radius_m: f64,
    pub inertia_factor: f64,
    /// Top of the metal and of the rock.
    pub core_radius_m: f64,
    pub refractory_radius_m: f64,
    /// Pressures above the load on the surface: the body's own weight.
    pub central_pa: f64,
    pub core_pa: f64,
    pub refractory_pa: f64,
}

impl Body {
    /// Regions innermost first, each with its inner and outer sample. A
    /// region is water, metal or rock by whichever it mostly holds.
    fn new<'a>(
        mass_kg: f64,
        inertia_kg_m2: f64,
        regions: impl Iterator<Item = (&'a Inventory, Sample, Sample)>,
    ) -> Self {
        let regions: Vec<_> = regions.collect();
        let centre = regions[0].1;
        let surface = regions[regions.len() - 1].2;
        // A layer that is absent ends where the one beneath it does.
        let mut core = centre;
        let mut refractory = centre;
        for (inventory, _, outer) in &regions {
            let [metal, rock, water] = [Material::Metal, Material::Silicate, Material::Water]
                .map(|m| inventory.mass_kg(m));
            if water > metal.max(rock) {
                continue;
            }
            refractory = *outer;
            if metal > rock {
                core = *outer;
            }
        }
        Self {
            radius_m: surface.radius_m,
            inertia_factor: inertia_kg_m2 / (mass_kg * surface.radius_m.powi(2)),
            core_radius_m: core.radius_m,
            refractory_radius_m: refractory.radius_m,
            central_pa: centre.pressure_pa - surface.pressure_pa,
            core_pa: core.pressure_pa - surface.pressure_pa,
            refractory_pa: refractory.pressure_pa - surface.pressure_pa,
        }
    }

    /// The seven numbers a cell interpolates, as `Table::solid` reads them.
    fn values(&self) -> [f64; 7] {
        [
            self.radius_m.ln(),
            self.inertia_factor,
            (self.core_radius_m / self.radius_m).powi(3),
            (self.refractory_radius_m / self.radius_m).powi(3),
            self.central_pa.ln(),
            self.core_pa / self.central_pa,
            self.refractory_pa / self.central_pa,
        ]
    }
}

/// A cell from its seven numbers, kept inside what a body can be: a fill
/// continued along a slope may leave it.
fn solid_cell(v: [f64; 7], reason: Reason) -> SolidCell {
    let refractory_volume = v[3].clamp(0.0, 1.0);
    let core_pressure = v[5].clamp(0.0, 1.0);
    SolidCell {
        ln_radius_m: v[0] as f32,
        inertia_factor: v[1] as f32,
        interior: Interior {
            core_volume: v[2].clamp(0.0, refractory_volume) as f32,
            refractory_volume: refractory_volume as f32,
            ln_central_pa: v[4] as f32,
            core_pressure: core_pressure as f32,
            refractory_pressure: v[6].clamp(0.0, core_pressure) as f32,
        }
        .stored(),
        reason,
    }
}

/// One bare condensed body.
pub(super) fn solve_condensed(
    mass_earth: f64,
    metal_share: f64,
    water_share: f64,
    temperature_k: f64,
) -> Result<Body, String> {
    let mass = mass_earth * EARTH_MASS_KG;
    let refractory = mass * (1.0 - water_share);
    let inventory = Inventory::new(&[
        (Material::Metal, refractory * metal_share),
        (Material::Silicate, refractory * (1.0 - metal_share)),
        (Material::Water, mass * water_share),
    ])
    .map_err(|e| format!("{e:?}"))?;
    let controls = dry::Controls::default();
    if water_share == 0.0 {
        dry::solve(&inventory, 0.0, controls).map(|s| {
            Body::new(
                mass,
                s.inertia_kg_m2(),
                s.regions()
                    .iter()
                    .map(|r| (r.inventory(), r.inner(), r.outer())),
            )
        })
    } else {
        water::solve(
            &inventory,
            temperature_k,
            water_load_pa(temperature_k),
            controls,
        )
        .map(|s| {
            Body::new(
                mass,
                s.inertia_kg_m2(),
                s.regions()
                    .iter()
                    .map(|r| (r.inventory(), r.inner(), r.outer())),
            )
        })
    }
    .map_err(|e| {
        let text = format!("{e:?}");
        text.split(['(', '{', ' ']).next().unwrap().to_owned()
    })
}

/// What the table keeps of one envelope solve.
#[derive(Clone, Copy, Debug)]
pub(super) struct Solved {
    /// One bar, or the loaded condensed boundary where the gas stays below it.
    pub radius_m: f64,
    pub core_radius_m: f64,
    /// Where the gas meets the core.
    pub base_pa: f64,
    /// Whole-body moment of inertia over mass and `radius_m` squared.
    pub inertia_factor: f64,
    /// The gas's alone, over its own mass and `radius_m` squared.
    pub gas_inertia_factor: f64,
}

/// An H/He envelope on the reference core.
pub(super) fn solve_envelope(
    mass_earth: f64,
    light_fraction: f64,
    equilibrium_k: f64,
    intrinsic_k: f64,
) -> Result<Solved, String> {
    solve_envelope_over(
        mass_earth,
        light_fraction,
        equilibrium_k,
        intrinsic_k,
        REFERENCE_CORE_METAL,
        0.0,
    )
}

/// An H/He envelope over a core of this metal share of its refractory mass
/// and this water share.
pub(super) fn solve_envelope_over(
    mass_earth: f64,
    light_fraction: f64,
    equilibrium_k: f64,
    intrinsic_k: f64,
    metal_share: f64,
    water_share: f64,
) -> Result<Solved, String> {
    let mass = mass_earth * EARTH_MASS_KG;
    let heavy = mass * (1.0 - light_fraction);
    let light = mass * light_fraction;
    let refractory = heavy * (1.0 - water_share);
    let retained = Inventory::new(&[
        (Material::Metal, refractory * metal_share),
        (Material::Silicate, refractory * (1.0 - metal_share)),
        (Material::Water, heavy * water_share),
        (Material::Hydrogen, light * (1.0 - REFERENCE_HELIUM)),
        (Material::Helium, light * REFERENCE_HELIUM),
    ])
    .map_err(|e| format!("{e:?}"))?;
    envelope::solve(
        &retained,
        &Inventory::default(),
        equilibrium_k,
        intrinsic_k,
        coupled::Controls::default(),
    )
    .map(|(interior, gas)| {
        let radius_m = gas.one_bar_radius_m().unwrap_or(interior.radius_m());
        Solved {
            radius_m,
            core_radius_m: interior.radius_m(),
            base_pa: gas.regions()[0].inner().pressure_pa,
            inertia_factor: gas.inertia_kg_m2 / (mass * radius_m.powi(2)),
            gas_inertia_factor: (gas.inertia_kg_m2 - interior.inertia_kg_m2())
                / (light * radius_m.powi(2)),
        }
    })
    .map_err(|e| {
        let text = format!("{e:?}");
        text.split(['{', '[']).next().unwrap().trim().to_owned()
    })
}

/// The retired closed-form envelope radius (Lopez and Fortney 2014 thickness,
/// blended into a self-compressed giant). Only refused cells use it, shifted
/// to meet their solved neighbours.
fn surrogate_radius_m(
    mass_earth: f64,
    fraction: f64,
    condensed_radius_m: f64,
    equilibrium_k: f64,
    intrinsic_k: f64,
) -> f64 {
    let flux = 4.0 * STEFAN_BOLTZMANN * equilibrium_k.powi(4) / 0.7 / EARTH_REFERENCE_FLUX_W_M2;
    // Invert the intrinsic-heat trend for the age the fit expects.
    let age = 4.57 * (intrinsic_k / (100.0 * (mass_earth / 317.83).powf(0.215))).powf(-10.0);
    let thickness = EARTH_REFERENCE_RADIUS_M
        * ENVELOPE_THICKNESS_EARTH
        * mass_earth
            .clamp(ENVELOPE_FIT_MASS_RANGE[0], ENVELOPE_FIT_MASS_RANGE[1])
            .powf(ENVELOPE_MASS_EXPONENT)
        * (fraction.min(ENVELOPE_FIT_MAX_FRACTION) / 0.05).powf(ENVELOPE_FRACTION_EXPONENT)
        * flux
            .clamp(ENVELOPE_FIT_FLUX_RANGE[0], ENVELOPE_FIT_FLUX_RANGE[1])
            .powf(ENVELOPE_FLUX_EXPONENT)
        * (age.clamp(ENVELOPE_FIT_AGE_RANGE[0], ENVELOPE_FIT_AGE_RANGE[1]) / 5.0)
            .powf(ENVELOPE_AGE_EXPONENT);
    let giant_radius = (EARTH_REFERENCE_RADIUS_M
        * GIANT_RADIUS_EARTH
        * (mass_earth / GIANT_REFERENCE_MASS_EARTH).cbrt()
        * ((1.0 + GIANT_REFERENCE_MASS_EARTH / GIANT_TURNOVER_MASS_EARTH)
            / (1.0 + mass_earth / GIANT_TURNOVER_MASS_EARTH))
            .powf(1.0 / 3.0 - GIANT_HIGH_MASS_EXPONENT))
    .max(condensed_radius_m);
    let capacity = giant_radius - condensed_radius_m;
    let thickness = if capacity > 0.0 {
        thickness / (1.0 + (thickness / capacity).powi(4)).powf(0.25)
    } else {
        0.0
    };
    let t = ((fraction - GIANT_BLEND_FRACTION_RANGE[0])
        / (GIANT_BLEND_FRACTION_RANGE[1] - GIANT_BLEND_FRACTION_RANGE[0]))
        .clamp(0.0, 1.0);
    let blend = t * t * (3.0 - 2.0 * t);
    (condensed_radius_m + thickness) * (1.0 - blend) + giant_radius * blend
}

/// Refused cells take their line's solved neighbours, one axis at a time in
/// the given order: interpolated between two, else continued along the slope
/// of the nearest pair. Each value is returned with whether it was filled.
fn fill<const N: usize>(
    grid: &Grid,
    solved: &[Option<[f64; N]>],
    axes: &[usize],
) -> Vec<([f64; N], bool)> {
    let mut cells: Vec<Option<([f64; N], bool)>> =
        solved.iter().map(|s| s.map(|v| (v, false))).collect();
    for &axis in axes {
        let snapshot = cells.clone();
        for (index, cell) in cells.iter_mut().enumerate() {
            if cell.is_some() {
                continue;
            }
            let at = grid.cell(index);
            let along = |offset: isize| -> Option<(f64, [f64; N])> {
                let i = at[axis] as isize + offset;
                if i < 0 || i >= grid.knots[axis].len() as isize {
                    return None;
                }
                let mut other = at;
                other[axis] = i as usize;
                snapshot[grid.index(other)].map(|(v, _)| (grid.knots[axis][i as usize], v))
            };
            let reach = grid.knots[axis].len() as isize;
            let x = grid.knots[axis][at[axis]];
            let side = |step: isize| {
                let first = (1..reach).find(|d| along(step * d).is_some())?;
                Some((
                    along(step * first)?,
                    (first + 1..reach).find_map(|d| along(step * d)),
                ))
            };
            let line = |(x0, a): (f64, [f64; N]), (x1, b): (f64, [f64; N])| {
                let u = (x - x0) / (x1 - x0);
                std::array::from_fn(|i| a[i] + u * (b[i] - a[i]))
            };
            *cell = match (side(-1), side(1)) {
                (Some((below, _)), Some((above, _))) => Some(line(below, above)),
                (Some((near, Some(far))), None) | (None, Some((near, Some(far)))) => {
                    Some(line(far, near))
                }
                (Some((near, None)), None) | (None, Some((near, None))) => Some(near.1),
                (None, None) => None,
            }
            .map(|v| (v, true));
        }
    }
    cells
        .into_iter()
        .map(|c| c.expect("every line has a solved cell"))
        .collect()
}

/// Unknown cells take the mean of their already known neighbours, ring by
/// ring outwards, keeping `fade` of it per ring after the first.
fn spread(grid: &Grid, mut values: Vec<Option<f64>>, fade: f64) -> Vec<f64> {
    let mut ring = 0;
    while values.iter().any(Option::is_none) {
        ring += 1;
        // The first ring meets the solved cells exactly.
        let fade = if ring == 1 { 1.0 } else { fade };
        let snapshot = values.clone();
        let mut progressed = false;
        for (index, value) in values.iter_mut().enumerate() {
            if value.is_some() {
                continue;
            }
            let at = grid.cell(index);
            let mut sum = 0.0;
            let mut count = 0;
            for axis in 0..4 {
                for step in [-1_isize, 1] {
                    let i = at[axis] as isize + step;
                    if i < 0 || i >= grid.knots[axis].len() as isize {
                        continue;
                    }
                    let mut other = at;
                    other[axis] = i as usize;
                    if let Some(known) = snapshot[grid.index(other)] {
                        sum += known;
                        count += 1;
                    }
                }
            }
            if count > 0 {
                *value = Some(fade * sum / f64::from(count));
                progressed = true;
            }
        }
        assert!(progressed, "no solved envelope cell to anchor a fill");
    }
    values.into_iter().map(Option::unwrap).collect()
}

/// Refused envelope cells take the surrogate, shifted by the mean offset of
/// their already known neighbours so the table has no step at the edge of
/// the solved region. The shift fades with distance: far from any solved
/// cell the surrogate stands on its own calibration. Base pressure does the
/// same against the weight of a thin shell, and the gas's inertia factor
/// takes its neighbours'.
fn fill_envelope(
    grid: &Grid,
    table: &Table,
    solved: &[Option<Solved>],
    reasons: &[Reason],
) -> Vec<EnvelopeCell> {
    let bare: Vec<f64> = (0..grid.len())
        .map(|index| {
            let [mass, fraction, ..] = envelope_point(grid, index);
            reference_core_radius_m(table, mass, fraction)
        })
        .collect();
    let surrogate: Vec<f64> = (0..grid.len())
        .map(|index| {
            let [mass, fraction, equilibrium, intrinsic] = envelope_point(grid, index);
            (bare[index] / surrogate_radius_m(mass, fraction, bare[index], equilibrium, intrinsic))
                .ln()
        })
        .collect();
    let thin_shell: Vec<f64> = (0..grid.len())
        .map(|index| {
            let [mass, fraction, ..] = envelope_point(grid, index);
            (crate::units::GRAVITATIONAL_CONSTANT * (mass * EARTH_MASS_KG).powi(2) * fraction
                / (4.0 * std::f64::consts::PI * bare[index].powi(4)))
            .ln()
        })
        .collect();
    let offset = spread(
        grid,
        (0..grid.len())
            .map(|i| solved[i].map(|s| (bare[i] / s.radius_m).ln() - surrogate[i]))
            .collect(),
        ANCHOR_FADE,
    );
    let base_offset = spread(
        grid,
        (0..grid.len())
            .map(|i| solved[i].map(|s| s.base_pa.ln() - thin_shell[i]))
            .collect(),
        ANCHOR_FADE,
    );
    let gas_inertia = spread(
        grid,
        solved
            .iter()
            .map(|s| s.map(|s| s.gas_inertia_factor))
            .collect(),
        1.0,
    );
    let [low, high] = [CORE_LOADS_PA[0], CORE_LOADS_PA[CORE_LOADS_PA.len() - 1]].map(f64::ln);
    let mut cells: Vec<EnvelopeCell> = (0..grid.len())
        .map(|index| EnvelopeCell {
            // A surrogate may not claim a body smaller than its bare core, nor
            // one beyond the puffiest the solver represents.
            compactness: match solved[index] {
                Some(s) => bare[index] / s.radius_m,
                None => (surrogate[index] + offset[index])
                    .exp()
                    .clamp(MIN_COMPACTNESS, 1.0),
            } as f32,
            ln_base_pa: match solved[index] {
                Some(s) => s.base_pa.ln(),
                None => thin_shell[index] + base_offset[index],
            }
            .clamp(low, high) as f32,
            gas_inertia_factor: gas_inertia[index] as f32,
            reason: reasons[index],
        })
        .collect();
    // Heating a body never shrinks it. Walking away from the solved cells
    // along each temperature axis, a refused cell is no more compact than
    // its cooler neighbour and no puffier than its warmer one. On the hot
    // side this carries the solver's last answer into the region where
    // weakly bound gas inflates without limit, instead of falling back to
    // the surrogate. Lines without a solved cell keep the faded anchor.
    for axis in [2, 3] {
        for hotter in [true, false] {
            let mut bound: Vec<Option<f32>> = vec![None; cells.len()];
            let order: Box<dyn Iterator<Item = usize>> = if hotter {
                Box::new(0..cells.len())
            } else {
                Box::new((0..cells.len()).rev())
            };
            for index in order {
                let mut previous = grid.cell(index);
                let edge = if hotter {
                    0
                } else {
                    grid.knots[axis].len() - 1
                };
                if previous[axis] != edge {
                    previous[axis] = if hotter {
                        previous[axis] - 1
                    } else {
                        previous[axis] + 1
                    };
                    bound[index] = bound[grid.index(previous)];
                }
                let cell = &mut cells[index];
                if cell.reason == Reason::Solved {
                    bound[index] = Some(cell.compactness);
                } else if let Some(limit) = bound[index] {
                    cell.compactness = if hotter {
                        cell.compactness.min(limit)
                    } else {
                        cell.compactness.max(limit)
                    };
                    bound[index] = Some(cell.compactness);
                }
            }
        }
    }
    cells
}

/// Mass in Earth masses, H/He share and both temperatures of one cell.
fn envelope_point(grid: &Grid, index: usize) -> [f64; 4] {
    let [mass, fraction, equilibrium, intrinsic] = grid.point(grid.cell(index));
    [mass.exp(), fraction.exp(), equilibrium, intrinsic]
}

const MIN_COMPACTNESS: f64 = 0.01;
/// Share of a neighbour's offset from the surrogate that a refused cell keeps.
const ANCHOR_FADE: f64 = 0.5;

/// The unloaded reference core: what compactness is stored against.
pub(super) fn reference_core_radius_m(table: &Table, mass_earth: f64, light_fraction: f64) -> f64 {
    table
        .core(
            mass_earth * (1.0 - light_fraction) * EARTH_MASS_KG,
            REFERENCE_CORE_METAL,
            0.0,
            0.0,
        )
        .radius_m
}

/// The envelope solver's heavy layers under this load.
pub(super) fn solve_core(
    mass_earth: f64,
    metal_share: f64,
    water_share: f64,
    load_pa: f64,
) -> Result<Body, String> {
    let mass = mass_earth * EARTH_MASS_KG;
    let refractory = mass * (1.0 - water_share);
    let heavy = Inventory::new(&[
        (Material::Metal, refractory * metal_share),
        (Material::Silicate, refractory * (1.0 - metal_share)),
        (Material::Water, mass * water_share),
    ])
    .map_err(|e| format!("{e:?}"))?;
    envelope::loaded_core(&heavy, load_pa, dry::Controls::default())
        .map(|(regions, inertia_kg_m2)| {
            Body::new(
                mass,
                inertia_kg_m2,
                regions
                    .iter()
                    .map(|r| (r.inventory(), r.inner(), r.outer())),
            )
        })
        .map_err(|e| {
            let text = format!("{e:?}");
            text.split(['{', '[']).next().unwrap().trim().to_owned()
        })
}

/// Cells from their solves: refused ones are filled along `axes` and keep
/// the solver's reason.
fn solid_cells(grid: &Grid, solved: Vec<Result<Body, String>>, axes: &[usize]) -> Vec<SolidCell> {
    let reasons: Vec<Reason> = solved
        .iter()
        .map(|s| s.as_ref().err().map_or(Reason::Solved, |e| Reason::of(e)))
        .collect();
    let values: Vec<_> = solved
        .into_iter()
        .map(|s| s.ok().map(|body| body.values()))
        .collect();
    fill(grid, &values, axes)
        .into_iter()
        .zip(reasons)
        .map(|((v, _), reason)| solid_cell(v, reason))
        .collect()
}

/// The condensed half of the current resource, which no envelope or core
/// change touches. Only a resource of the current layout and grids.
fn embedded_condensed() -> Option<Vec<SolidCell>> {
    Table::decode(BYTES).map(|table| table.condensed)
}

fn tally(report: &mut String, title: &str, outcomes: &[Result<(), String>], seconds: f64) {
    let mut reasons = BTreeMap::<&str, usize>::new();
    for refusal in outcomes.iter().filter_map(|o| o.as_ref().err()) {
        *reasons.entry(refusal).or_default() += 1;
    }
    let refused: usize = reasons.values().sum();
    let _ = writeln!(
        report,
        "## {title}\n\n- Cells: {}, solved: {} ({:.1}%), filled: {refused}\n- Wall time: {seconds:.1} s\n",
        outcomes.len(),
        outcomes.len() - refused,
        100.0 * (outcomes.len() - refused) as f64 / outcomes.len().max(1) as f64,
    );
    let mut reasons: Vec<_> = reasons.into_iter().collect();
    reasons.sort_by_key(|r| std::cmp::Reverse(r.1));
    for (reason, count) in reasons {
        let _ = writeln!(report, "- {count}: {reason}");
    }
    report.push('\n');
}

/// Where each refusal sits: its share of the cells and the span of every
/// axis it touches, as the knots are stored (masses and gas shares back in
/// plain units).
fn chart(report: &mut String, grid: &Grid, cells: &[Reason], axes: [(&str, bool); 4]) {
    let _ = writeln!(
        report,
        "| Reason | Cells | {} |\n| --- | --- | --- | --- | --- | --- |",
        axes.map(|a| a.0).join(" | ")
    );
    for reason in Reason::ALL {
        let at: Vec<[usize; 4]> = (0..cells.len())
            .filter(|i| cells[*i] == reason)
            .map(|i| grid.cell(i))
            .collect();
        if at.is_empty() || reason == Reason::Solved {
            continue;
        }
        let spans: Vec<String> = (0..4)
            .map(|axis| {
                let value = |i: usize| {
                    let knot = grid.knots[axis][i];
                    if axes[axis].1 { knot.exp() } else { knot }
                };
                format!(
                    "{:.3e} to {:.3e}",
                    value(at.iter().map(|c| c[axis]).min().unwrap()),
                    value(at.iter().map(|c| c[axis]).max().unwrap())
                )
            })
            .collect();
        let _ = writeln!(
            report,
            "| {} | {} ({:.1}%) | {} |",
            reason.label(),
            at.len(),
            100.0 * at.len() as f64 / cells.len() as f64,
            spans.join(" | ")
        );
    }
    report.push('\n');
}

/// `scripts/prepare-structure-table.sh`. Writes the embedded resource and
/// data/tmp/structure-table.md.
#[test]
#[ignore = "offline preparation: rewrites the embedded structure table"]
fn prepare_structure_table() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut report = "# Structure table\n\n".to_owned();

    let grid = Grid::condensed();
    let start = Instant::now();
    let reuse = std::env::var_os("NS_TABLE_REUSE_CONDENSED").and_then(|_| embedded_condensed());
    let solved = parallel(if reuse.is_some() { 0 } else { grid.len() }, |index| {
        let at = grid.cell(index);
        let [mass, metal, water_share, temperature] = grid.point(at);
        // Dry bodies do not depend on the water temperature, and pure water
        // has no refractory mix: solve each once.
        if (water_share == 0.0 && at[3] > 0) || (water_share == 1.0 && at[1] > 0) {
            return Err("duplicate".to_owned());
        }
        solve_condensed(mass.exp(), metal, water_share, temperature)
    });
    let mut solved: Vec<Result<Body, String>> = solved;
    for index in 0..solved.len() {
        let mut at = grid.cell(index);
        if grid.knots[2][at[2]] == 0.0 {
            at[3] = 0;
        } else if grid.knots[2][at[2]] == 1.0 {
            at[1] = 0;
        }
        solved[index] = solved[grid.index(at)].clone();
    }
    let outcomes: Vec<_> = solved.iter().map(|s| s.clone().map(|_| ())).collect();
    tally(
        &mut report,
        "Condensed bodies",
        &outcomes,
        start.elapsed().as_secs_f64(),
    );
    // Temperature first: radius barely moves along it.
    let condensed = reuse.unwrap_or_else(|| solid_cells(&grid, solved, &[3, 0, 2, 1]));
    chart(
        &mut report,
        &grid,
        &condensed.iter().map(|c| c.reason).collect::<Vec<_>>(),
        [
            ("Mass (Earth)", true),
            ("Metal share", false),
            ("Water share", false),
            ("Temperature (K)", false),
        ],
    );

    let grid = Grid::core();
    let start = Instant::now();
    let solved = parallel(grid.len(), |index| {
        let mut at = grid.cell(index);
        // Pure water has no refractory mix.
        if grid.knots[2][at[2]] == 1.0 {
            at[1] = 0;
        }
        let [mass, metal, water_share, load] = grid.point(at);
        solve_core(mass.exp(), metal, water_share, load.exp())
    });
    let outcomes: Vec<_> = solved.iter().map(|s| s.clone().map(|_| ())).collect();
    tally(
        &mut report,
        "Cores under an envelope",
        &outcomes,
        start.elapsed().as_secs_f64(),
    );
    // Load first: a refused cell is nearest its own body under another load.
    let core = solid_cells(&grid, solved, &[3, 0, 2, 1]);
    chart(
        &mut report,
        &grid,
        &core.iter().map(|c| c.reason).collect::<Vec<_>>(),
        [
            ("Mass (Earth)", true),
            ("Metal share", false),
            ("Water share", false),
            ("Load (Pa)", true),
        ],
    );

    let grid = Grid::envelope();
    let partial = Table::new(
        condensed.clone(),
        vec![
            EnvelopeCell {
                compactness: 1.0,
                ln_base_pa: 0.0,
                gas_inertia_factor: 1.0,
                reason: Reason::Other,
            };
            grid.len()
        ],
        core.clone(),
    );
    let start = Instant::now();
    let solved = parallel(grid.len(), |index| {
        let [mass, fraction, equilibrium, intrinsic] = envelope_point(&grid, index);
        solve_envelope(mass, fraction, equilibrium, intrinsic)
    });
    let outcomes: Vec<_> = solved.iter().map(|s| s.clone().map(|_| ())).collect();
    tally(
        &mut report,
        "Envelopes",
        &outcomes,
        start.elapsed().as_secs_f64(),
    );
    let mut cells =
        "mass_earth\tlight_fraction\tequilibrium_k\tintrinsic_k\tradius_m\tbase_pa\tinertia_factor\trefusal\n".to_owned();
    for (index, outcome) in solved.iter().enumerate() {
        let [mass, fraction, equilibrium, intrinsic] = envelope_point(&grid, index);
        let _ = writeln!(
            cells,
            "{mass:.6e}\t{fraction:.6e}\t{equilibrium:.5e}\t{intrinsic:.5e}\t{}\t{}",
            outcome.as_ref().map_or("\t\t".to_owned(), |v| format!(
                "{:.6e}\t{:.4e}\t{:.5}",
                v.radius_m, v.base_pa, v.inertia_factor
            )),
            outcome.as_ref().err().map_or("", |e| e.as_str()),
        );
    }
    let reasons: Vec<Reason> = solved
        .iter()
        .map(|s| s.as_ref().err().map_or(Reason::Solved, |e| Reason::of(e)))
        .collect();
    let values: Vec<_> = solved.into_iter().map(Result::ok).collect();
    let envelope = fill_envelope(&grid, &partial, &values, &reasons);
    chart(
        &mut report,
        &grid,
        &reasons,
        [
            ("Mass (Earth)", true),
            ("H/He share", true),
            ("Equilibrium (K)", false),
            ("Intrinsic (K)", false),
        ],
    );

    let table = Table::new(condensed, envelope, core);
    let bytes = table.encode();
    assert_eq!(Table::decode(&bytes).as_ref(), Some(&table));
    let _ = writeln!(
        report,
        "## Resource\n\n- {} bytes, checksum 0x{:016x}",
        bytes.len(),
        u64::from_le_bytes(bytes[bytes.len() - 8..].try_into().unwrap())
    );
    std::fs::create_dir_all(root.join("resources")).unwrap();
    std::fs::write(root.join("resources/structure-table.bin"), bytes).unwrap();
    let tmp = root.join("../../data/tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("structure-table.md"), &report).unwrap();
    std::fs::write(tmp.join("structure-table-envelope.tsv"), cells).unwrap();
    eprintln!("{report}");
}
