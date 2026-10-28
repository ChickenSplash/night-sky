use super::*;
use crate::{
    system::atmosphere::Gas,
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K},
};

fn gas(z: f64) -> Inventory {
    Inventory::new(&[
        (Material::Hydrogen, 0.75 * (1.0 - z)),
        (Material::Helium, 0.25 * (1.0 - z)),
        (Material::CarbonDioxide, 0.5 * z),
        (Material::Nitrogen, 0.48 * z),
        (Material::Methane, 0.01 * z),
        (Material::Argon, 0.01 * z),
    ])
    .unwrap()
}

fn ideal_hhe(p: f64, t: f64) -> f64 {
    p / (BOLTZMANN_J_K / ATOMIC_MASS_KG
        * t
        * (0.75 / Gas::Hydrogen.molecular_mass_u() + 0.25 / Gas::Helium.molecular_mass_u()))
}

#[test]
fn pure_endpoint_is_exact_and_absent_species_do_not_restrict_it() {
    let pure = Mixture::new(&gas(0.0)).unwrap();
    assert_eq!(pure.density_kg_m3(1e12, 20000.0, 1000.0), Ok(1000.0));
    assert_eq!(
        pure.density_kg_m3(1e5, 300.0, f64::NAN),
        Err(Error::InvalidDensity)
    );
}

#[test]
fn low_pressure_mixture_has_correct_molecular_weight_and_continuous_enrichment() {
    let p = 1.0;
    let t = 400.0;
    let mut previous = 0.0;
    for i in 0..=100 {
        let z = i as f64 * 0.000499;
        let inventory = gas(z);
        let mix = Mixture::new(&inventory).unwrap();
        let density = mix.density_kg_m3(p, t, ideal_hhe(p, t)).unwrap();
        let reciprocal_mu: f64 = Material::ALL
            .into_iter()
            .filter_map(|m| {
                m.properties()
                    .gas
                    .map(|g| inventory.mass_kg(m) / g.molecular_mass_u())
            })
            .sum();
        let ideal = p / (BOLTZMANN_J_K / ATOMIC_MASS_KG * t * reciprocal_mu);
        assert!((density / ideal - 1.0).abs() < 1e-7);
        assert!(density > previous);
        previous = density;
    }
}

#[test]
fn mixture_uses_specific_volume_not_density_averaging() {
    let inventory = gas(0.04);
    let mix = Mixture::new(&inventory).unwrap();
    let (p, t, hhe) = (1e7, 400.0, 7.0);
    let mut volume = 0.96 / hhe;
    for m in [
        Material::CarbonDioxide,
        Material::Nitrogen,
        Material::Methane,
        Material::Argon,
    ] {
        volume += inventory.mass_kg(m) / molecular::density_kg_m3(m, p, t).unwrap();
    }
    assert!((mix.density_kg_m3(p, t, hhe).unwrap() * volume - 1.0).abs() < 1e-14);
}

#[test]
fn composition_and_component_refusals_are_preserved() {
    assert!(Mixture::new(&gas(0.05)).is_ok());
    assert_eq!(Mixture::new(&gas(0.050001)), Err(Error::CompositionDomain));
    assert_eq!(Mixture::new(&gas(0.1)), Err(Error::CompositionDomain));
    let water = gas(0.01)
        .checked_add(&Inventory::new(&[(Material::Water, 0.001)]).unwrap())
        .unwrap();
    assert_eq!(Mixture::new(&water), Err(Error::CompositionDomain));
    let mix = Mixture::new(&gas(0.04)).unwrap();
    assert!(matches!(
        mix.density_kg_m3(1e9, 400.0, 10.0),
        Err(Error::Component(_))
    ));
    assert!(matches!(
        mix.density_kg_m3(1000.0, 100.0, 0.01),
        Err(Error::Component(_))
    ));
    let tiny = gas(0.0)
        .checked_add(&Inventory::new(&[(Material::CarbonDioxide, 1e-25)]).unwrap())
        .unwrap();
    assert!(matches!(
        Mixture::new(&tiny).unwrap().density_kg_m3(1e9, 400.0, 10.0),
        Err(Error::Component(_))
    ));
}

#[test]
fn overlap_with_the_original_passive_trace_model_is_bounded() {
    let z = crate::system::structure::dry::protected::MAX_TRACE_FRACTION;
    let mix = Mixture::new(&gas(z)).unwrap();
    for (p, t) in [(1000.0, 350.0), (1e5, 400.0), (1e7, 600.0)] {
        let hhe = ideal_hhe(p, t);
        let corrected = mix.density_kg_m3(p, t, hhe).unwrap();
        assert!((corrected / hhe - 1.0).abs() < 0.0031);
    }
}
