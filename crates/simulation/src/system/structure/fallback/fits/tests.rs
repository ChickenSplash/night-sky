use super::*;

fn radius(material: Material, mass: f64) -> f64 {
    pure_radius_earth(material, mass).unwrap()
}

fn gas_radius(mass: f64, fraction: f64, flux: f64, age: f64) -> (f64, bool) {
    let mut clamped = false;
    let r = one_bar(
        mass * EARTH_MASS_KG,
        fraction * mass * EARTH_MASS_KG,
        EARTH_REFERENCE_RADIUS_M,
        flux,
        age,
        &mut clamped,
    )
    .unwrap_or(EARTH_REFERENCE_RADIUS_M);
    (r, clamped)
}

#[test]
fn condensed_fits_keep_material_order_compression_and_continuation() {
    for mass in [0.002, 0.055, 1.0, 10.0, 20.0, 100.0] {
        let metal = radius(Material::Metal, mass);
        let rock = radius(Material::Silicate, mass);
        let water = radius(Material::Water, mass);
        assert!(metal < rock && rock < water);
        let doubled = radius(Material::Silicate, mass * 2.0);
        assert!(doubled > rock && doubled / rock < 2_f64.cbrt());
    }
    for material in [Material::Metal, Material::Silicate, Material::Water] {
        let left = radius(material, 20.0 - 1e-8);
        let right = radius(material, 20.0 + 1e-8);
        assert!((left - right).abs() < 1e-6);
        assert_eq!(
            radius(material, 100.0),
            radius(material, 20.0) * 5_f64.powf(CONDENSED_CONTINUATION_EXPONENT)
        );
    }
    assert_eq!(pure_radius_earth(Material::Nitrogen, 1.0), None);
}

#[test]
fn envelope_fit_preserves_stripping_blends_and_giant_turnover() {
    let (stripped, _) = gas_radius(5.0, 1e-12, 1361.0, 5.0);
    assert_eq!(stripped, EARTH_REFERENCE_RADIUS_M);
    for mass in [1.0, 20.0, 95.0, 318.0] {
        for fraction in [0.1, 0.2, 0.5] {
            let (left, _) = gas_radius(mass, fraction - 1e-8, 1361.0, 5.0);
            let (right, _) = gas_radius(mass, fraction + 1e-8, 1361.0, 5.0);
            assert!((left - right).abs() / EARTH_REFERENCE_RADIUS_M < 1e-4);
        }
    }
    assert!(gas_radius(318.0, 0.9, 1361.0, 5.0).0 > gas_radius(95.0, 0.9, 1361.0, 5.0).0);
    assert!(gas_radius(4000.0, 0.9, 1361.0, 5.0).0 < gas_radius(1000.0, 0.9, 1361.0, 5.0).0);
}

#[test]
fn envelope_heating_age_and_fit_bounds_remain_explicit() {
    let (old, clamped) = gas_radius(5.0, 0.05, 1361.0, 10.0);
    assert!(!clamped);
    assert!(gas_radius(5.0, 0.05, 1361.0, 0.1).0 > old);
    assert!(gas_radius(5.0, 0.05, 136100.0, 10.0).0 > old);
    for (mass, fraction, flux, age) in [
        (0.1, 0.05, 1361.0, 5.0),
        (5.0, 0.3, 1361.0, 5.0),
        (5.0, 0.05, 0.0, 5.0),
        (5.0, 0.05, 1361.0, 100.0),
    ] {
        assert!(gas_radius(mass, fraction, flux, age).1);
    }
}
