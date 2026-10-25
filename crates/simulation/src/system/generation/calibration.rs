//! Deterministic diagnostic census, not an observationally fitted population.
use super::*;
use crate::star::multiple::Hosted;
use crate::system::{
    atmosphere::GasAtmosphere,
    clearance::Policy,
    composition::Material,
    physical,
    shape::{MAX_FLATTENING, planet_shape},
};
use space_shared::Planet;
use std::collections::BTreeMap;

const SYSTEMS_PER_COMPONENT: usize = 256;

#[derive(Default)]
struct Sample {
    // Mass, radius, density, water, metal, H/He, escaped/source.
    values: Vec<[f64; 7]>,
    classes: BTreeMap<&'static str, usize>,
    caveats: BTreeMap<&'static str, usize>,
    no_air: usize,
    extended: usize,
    max_steps: u8,
    min_mass_floor: usize,
    max_gas_cap: usize,
}

impl Sample {
    fn add(&mut self, p: &Hosted) {
        let b = &p.body;
        let mass = b.structure.mass_kg();
        self.values.push([
            b.mass_earth,
            b.radius_km / EARTH_RADIUS_KM,
            b.structure.mean_density_kg_m3(),
            b.budget.retained().mass_kg(Material::Water) / mass,
            b.budget.retained().mass_kg(Material::Metal) / mass,
            b.structure.h_he_mass_fraction(),
            b.budget.escaped().total_mass_kg() / b.budget.source().total_mass_kg(),
        ]);
        *self.classes.entry(b.class.name()).or_default() += 1;
        for caveat in b.structure.approximations() {
            *self.caveats.entry(caveat.id()).or_default() += 1;
        }
        if let RetentionSolve::Converged { steps } = b.retention_solve {
            assert!(steps <= retention::MAX_STEPS);
            self.max_steps = self.max_steps.max(steps);
        }
        let source_mass = b.budget.source().total_mass_kg() / EARTH_MASS_KG;
        self.min_mass_floor += usize::from((source_mass / 0.0022 - 1.0).abs() < 1e-8);
        self.max_gas_cap += usize::from(source_mass >= 3899.99);
        match &p.environment.as_ref().unwrap().atmosphere {
            None => self.no_air += 1,
            Some(air) => self.extended += usize::from(air.extended),
        }
    }

    fn quantiles(&self, column: usize) -> [f64; 5] {
        let mut sorted: Vec<_> = self.values.iter().map(|v| v[column]).collect();
        sorted.sort_by(f64::total_cmp);
        [0.0, 0.05, 0.5, 0.95, 1.0].map(|q| sorted[((sorted.len() - 1) as f64 * q) as usize])
    }

    fn report(&self, name: &str) {
        eprintln!("{name}: n={} classes={:?}", self.values.len(), self.classes);
        for (i, name) in [
            "mass_E",
            "radius_E",
            "density_kg_m3",
            "water",
            "metal",
            "H_He",
            "escaped_source",
        ]
        .into_iter()
        .enumerate()
        {
            let values = self.quantiles(i).map(|v| format!("{v:.6e}")).join(", ");
            eprintln!("  {name}: min/p05/p50/p95/max=[{values}]");
        }
        eprintln!(
            "  no_air={} extended={} max_steps={} source_floor={} source_cap={} caveats={:?}",
            self.no_air,
            self.extended,
            self.max_steps,
            self.min_mass_floor,
            self.max_gas_cap,
            self.caveats
        );
    }
}

fn wire(p: &Hosted) -> Planet {
    Planet {
        id: p.id(),
        designation: p.id(),
        page: 0,
        orbit_index: p.index as u32,
        host: p.host.clone(),
        orbit: p.body.orbit,
        radius_km: p.body.radius_km,
        mass_earth: p.body.mass_earth,
        class: p.body.class.name().into(),
        environment: p.environment.clone(),
        inertia_factor: Some(p.body.structure.inertia_factor()),
        physical: Some(physical::to_data(&p.body)),
        resonance: p.body.resonance,
        spin: p.body.spin,
        task: None,
    }
}

fn check(p: &Hosted) {
    let planet = wire(p);
    physical::validate(&planet)
        .unwrap_or_else(|error| panic!("{}: {error}: {:?}", p.id(), planet.physical))
        .unwrap();
    let b = &p.body;
    assert!(b.mass_earth > 0.0 && b.mass_earth <= 3900.0 * (1.0 + 1e-12));
    assert!(b.radius_km > 0.0 && b.radius_km < 20.0 * EARTH_RADIUS_KM);
    for m in Material::ALL {
        let original = b.budget.source().mass_kg(m);
        let accounted = b.budget.retained().mass_kg(m) + b.budget.escaped().mass_kg(m);
        assert!(
            (accounted - original).abs() <= original * 1e-13,
            "{} {m:?}",
            p.id()
        );
    }
    let shape = planet_shape(&planet);
    assert!(shape.flattening() <= MAX_FLATTENING * (1.0 + 1e-12));
    let clearance = Policy::default().planet(&planet).unwrap();
    assert!(clearance.preferred_m > clearance.exclusion_m);
    assert!(clearance.exclusion_m > shape.equatorial);
    if let Some(data) = &p.environment.as_ref().unwrap().atmosphere {
        let air = GasAtmosphere::try_from(data).unwrap();
        let gm = b.mass_earth * crate::EARTH_SOLAR * crate::SUN_GM;
        let optics = air.optics(gm, b.radius_km * 1000.0).unwrap();
        for coefficient in optics
            .rayleigh_m_inv
            .into_iter()
            .chain(optics.gas_absorption_m_inv)
            .chain(optics.aerosol_scattering_m_inv)
            .chain([optics.aerosol_absorption_m_inv])
        {
            assert!(coefficient.is_finite() && coefficient >= 0.0);
        }
    }
}

#[test]
fn deterministic_population_census() {
    let mut samples = BTreeMap::<String, Sample>::new();
    let mut extremes = BTreeMap::<&str, (f64, String)>::new();
    let mut system_counts = [0; 4];
    for component in Component::ALL {
        for index in 0..SYSTEMS_PER_COMPONENT {
            let key = format!("composition-calibration:{component:?}:{index}");
            let system = galactic_bodies(&key, component);
            system_counts[system.stars.len() - 1] += 1;
            for p in &system.planets {
                check(p);
                let mass = system.mass(&p.host);
                let age = p.environment.as_ref().unwrap().age_gyr;
                for group in [
                    "all".to_owned(),
                    format!("component:{component:?}"),
                    format!("class:{}", p.body.class.name()),
                    format!(
                        "host:{}",
                        if mass < 0.5 {
                            "<0.5"
                        } else if mass < 1.5 {
                            "0.5-1.5"
                        } else {
                            ">=1.5"
                        }
                    ),
                    format!(
                        "age:{}",
                        if age < 0.2 {
                            "<0.2"
                        } else if age < 5.0 {
                            "0.2-5"
                        } else {
                            ">=5"
                        }
                    ),
                    format!("stars:{}", system.stars.len()),
                ] {
                    samples.entry(group).or_default().add(p);
                }
                for (name, value) in [
                    ("radius_max", p.body.radius_km / EARTH_RADIUS_KM),
                    ("density_max", p.body.structure.mean_density_kg_m3()),
                    (
                        "inverse_density_max",
                        1.0 / p.body.structure.mean_density_kg_m3(),
                    ),
                    (
                        "pressure_max",
                        p.environment
                            .as_ref()
                            .unwrap()
                            .atmosphere
                            .as_ref()
                            .map_or(0.0, |a| a.pressure_pa),
                    ),
                    (
                        "extent_radius_max",
                        p.environment
                            .as_ref()
                            .unwrap()
                            .atmosphere
                            .as_ref()
                            .map_or(0.0, |a| a.extent_m / (p.body.radius_km * 1000.0)),
                    ),
                ] {
                    let entry = extremes.entry(name).or_insert((0.0, String::new()));
                    if value > entry.0 {
                        *entry = (value, p.id());
                    }
                }
            }
        }
    }
    eprintln!("systems by 1/2/3/4 stars: {system_counts:?}; extremes={extremes:?}");
    for (name, sample) in &samples {
        sample.report(name);
    }
    for (_, id) in extremes.values() {
        let key = crate::star::multiple::system_key(id);
        let component = Component::ALL
            .into_iter()
            .find(|c| key.contains(&format!(":{c:?}:")))
            .unwrap();
        let system = galactic_bodies(key, component);
        let p = system.planets.iter().find(|p| p.id() == *id).unwrap();
        let b = &p.body;
        let env = p.environment.as_ref().unwrap();
        eprintln!(
            "outlier {id}: mass_E={:.6} radius_E={:.6} density={:.6} water={:.6} metal={:.6} H_He={:.6} host_Msun={:.6} age_Gyr={:.6} flux={:.6} AU={:.6} pressure_extent_ratio={:?}",
            b.mass_earth,
            b.radius_km / EARTH_RADIUS_KM,
            b.structure.mean_density_kg_m3(),
            b.budget.retained().mass_kg(Material::Water) / b.structure.mass_kg(),
            b.budget.retained().mass_kg(Material::Metal) / b.structure.mass_kg(),
            b.structure.h_he_mass_fraction(),
            system.mass(&p.host),
            env.age_gyr,
            env.irradiation_w_m2,
            b.orbit.semi_major_au,
            env.atmosphere.as_ref().map(|a| (
                a.pressure_pa,
                a.extent_m / (b.radius_km * 1000.0),
                a.extended
            ))
        );
    }
    let all = &samples["all"];
    for (column, edges) in [
        (0, &[0.01, 0.1, 1.0, 10.0, 100.0, 1000.0][..]),
        (1, &[0.2, 0.5, 1.0, 2.0, 4.0, 8.0, 12.0][..]),
        (2, &[500.0, 1000.0, 3000.0, 6000.0, 9000.0][..]),
    ] {
        let mut counts = vec![0; edges.len() + 1];
        for values in &all.values {
            counts[edges.partition_point(|&edge| values[column] >= edge)] += 1;
        }
        eprintln!("histogram column={column} edges={edges:?} counts={counts:?}");
    }
    assert!(all.values.len() > 10_000);
    assert_eq!(all.classes.len(), 6, "missing descriptive population");
    assert!(all.no_air > 0 && all.extended > 0);
    // Loose model-level alarms, not a fit to catalogue occurrence rates or an
    // exact snapshot of this seed set. Diagnostics above retain the full tails.
    assert!((0.01..2.0).contains(&all.quantiles(0)[2]));
    assert!((0.2..1.5).contains(&all.quantiles(1)[2]));
    assert!((1000.0..10000.0).contains(&all.quantiles(2)[2]));
    assert!(all.min_mass_floor < all.values.len() / 4);
    for count in all.classes.values() {
        let fraction = *count as f64 / all.values.len() as f64;
        assert!(
            (0.002..0.8).contains(&fraction),
            "collapsed population: {fraction}"
        );
    }
    assert!(
        samples["class:Water-rich planet"].quantiles(2)[2]
            < samples["class:Metal-rich planet"].quantiles(2)[2]
    );
    for group in [
        "host:<0.5",
        "host:0.5-1.5",
        "host:>=1.5",
        "age:<0.2",
        "age:0.2-5",
        "age:>=5",
        "stars:1",
        "stars:2",
        "stars:3",
        "stars:4",
    ] {
        assert!(samples[group].values.len() > 20, "under-sampled {group}");
    }
}

#[test]
fn reverse_order_and_parallel_regeneration_preserve_complete_planets() {
    let keys: Vec<_> = Component::ALL
        .into_iter()
        .flat_map(|component| {
            (0..8).map(move |i| {
                (
                    format!("composition-calibration:{component:?}:{i}"),
                    component,
                )
            })
        })
        .collect();
    let serial: Vec<_> = keys
        .iter()
        .map(|(k, c)| {
            galactic_bodies(k, *c)
                .planets
                .iter()
                .map(wire)
                .collect::<Vec<_>>()
        })
        .collect();
    std::thread::scope(|scope| {
        let jobs: Vec<_> = keys
            .chunks(14)
            .zip(serial.chunks(14))
            .rev()
            .map(|(keys, expected)| {
                scope.spawn(move || {
                    for ((key, component), expected) in keys.iter().zip(expected).rev() {
                        let actual = galactic_bodies(key, *component);
                        assert_eq!(
                            actual.planets.iter().map(wire).collect::<Vec<_>>(),
                            *expected
                        );
                        for p in &actual.planets {
                            check(p);
                        }
                    }
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
    });
}
