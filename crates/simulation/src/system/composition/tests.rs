use super::*;

fn close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-12 * a.abs().max(b.abs()), "{a} != {b}");
}

#[test]
fn catalogue_ids_and_gas_mapping_are_unique_and_round_trip() {
    for (i, material) in Material::ALL.into_iter().enumerate() {
        assert_eq!(Material::from_id(material.id()), Some(material));
        assert!(!Material::ALL[..i].iter().any(|m| m.id() == material.id()));
    }
    assert_eq!(Material::from_id("unobtainium"), None);
    for gas in Gas::ALL {
        assert_eq!(Material::from_gas(gas).properties().gas, Some(gas));
    }
    assert!(Material::Nitrogen.properties().condensed_fit.is_none());
}

#[test]
fn invalid_inventories_are_rejected_not_normalised() {
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Inventory::new(&[(Material::Metal, invalid)]).is_err());
        assert!(Inventory::default().scaled(invalid).is_err());
        assert!(Inventory::from_mass_fractions(invalid, &[(Material::Metal, 1.0)]).is_err());
    }
    assert!(Inventory::new(&[(Material::Metal, 0.0), (Material::Metal, 1.0)]).is_err());
    assert!(Inventory::new(&[(Material::Metal, f64::MAX), (Material::Water, f64::MAX)]).is_err());
    assert!(Inventory::from_mass_fractions(0.0, &[(Material::Metal, 1.0)]).is_err());
    assert!(Inventory::from_mass_fractions(1.0, &[]).is_err());
    assert!(Inventory::from_mass_fractions(1.0, &[(Material::Metal, 0.5)]).is_err());
    assert!(Inventory::from_mass_fractions(1.0, &[(Material::Metal, 1.01)]).is_err());
    let pure = Inventory::from_mass_fractions(10.0, &[(Material::Metal, 1.0 - 1e-10)]).unwrap();
    close(pure.total_mass_kg(), 10.0);
    assert_eq!(Inventory::default().mass_fraction(Material::Metal), None);
    assert_eq!(pure.mass_fraction(Material::Water), Some(0.0));
    assert!(pure.scaled(f64::MAX).is_err());
}

#[test]
fn molecular_conversion_distinguishes_mole_and_mass_fractions() {
    let mixture = GasComposition::new(&[(Gas::Hydrogen, 0.5), (Gas::Helium, 0.5)]).unwrap();
    let inventory = Inventory::from_gas_mass(&mixture, 100.0).unwrap();
    close(inventory.total_mass_kg(), 100.0);
    assert!(inventory.mass_fraction(Material::Helium).unwrap() > 0.66);
    let recovered = inventory.gas_composition().unwrap().unwrap();
    for gas in Gas::ALL {
        close(recovered.fraction(gas), mixture.fraction(gas));
    }
    assert_eq!(Inventory::default().gas_composition().unwrap(), None);
    assert!(
        Inventory::new(&[(Material::Metal, 1.0)])
            .unwrap()
            .gas_composition()
            .is_err()
    );
    assert!(Inventory::from_gas_mass(&mixture, -1.0).is_err());
    let all = GasComposition::new(&Gas::ALL.map(|gas| (gas, 0.125))).unwrap();
    let recovered = Inventory::from_gas_mass(&all, 1e25)
        .unwrap()
        .gas_composition()
        .unwrap()
        .unwrap();
    for gas in Gas::ALL {
        close(recovered.fraction(gas), all.fraction(gas));
    }
}

#[test]
fn allocations_and_escape_conserve_each_constituent() {
    let source = Inventory::new(&[
        (Material::Silicate, 100.0),
        (Material::Water, 20.0),
        (Material::Hydrogen, 5.0),
    ])
    .unwrap();
    let interior = Inventory::new(&[(Material::Silicate, 100.0)]).unwrap();
    let water = Inventory::new(&[(Material::Water, 15.0)]).unwrap();
    let gas = Inventory::new(&[(Material::Water, 5.0), (Material::Hydrogen, 2.0)]).unwrap();
    let mut budget = Budget::new(source, interior, water, gas).unwrap();
    close(budget.retained().total_mass_kg(), 122.0);
    close(budget.escaped().mass_kg(Material::Hydrogen), 3.0);
    for _ in 0..100 {
        budget
            .transfer(
                Reservoir::Gas,
                Reservoir::CondensedVolatiles,
                Material::Water,
                0.01,
            )
            .unwrap();
    }
    for material in Material::ALL {
        let allocated: f64 = [
            Reservoir::Interior,
            Reservoir::CondensedVolatiles,
            Reservoir::Gas,
        ]
        .iter()
        .map(|&r| budget.reservoir(r).mass_kg(material))
        .sum();
        close(allocated, budget.retained().mass_kg(material));
        close(
            allocated + budget.escaped().mass_kg(material),
            source.mass_kg(material),
        );
    }
    let before = budget;
    assert!(
        budget
            .transfer(Reservoir::Gas, Reservoir::Interior, Material::Hydrogen, 3.0)
            .is_err()
    );
    assert_eq!(budget, before);
    assert!(
        budget
            .transfer(Reservoir::Gas, Reservoir::Gas, Material::Water, 0.0)
            .is_err()
    );
    assert!(
        budget
            .transfer(
                Reservoir::Gas,
                Reservoir::Interior,
                Material::Water,
                f64::NAN
            )
            .is_err()
    );
}

#[test]
fn no_absolute_tolerance_can_create_a_trace_gas_or_massless_planet() {
    let rock = Inventory::new(&[(Material::Silicate, 1e24)]).unwrap();
    let trace = Inventory::new(&[(Material::Argon, 1e-30)]).unwrap();
    assert_eq!(
        Budget::new(rock, rock, Inventory::default(), trace),
        Err(CompositionError::BudgetExceeded(Material::Argon))
    );
    assert!(
        Budget::new(
            rock,
            Inventory::default(),
            Inventory::default(),
            Inventory::default()
        )
        .is_err()
    );
    let source = Inventory::new(&[(Material::Argon, 1.0)]).unwrap();
    let too_much = Inventory::new(&[(Material::Argon, 1.0 + 1e-12)]).unwrap();
    assert!(Budget::new(source, too_much, Inventory::default(), Inventory::default()).is_err());
}

#[test]
fn sub_precision_transfers_fail_without_creating_trace_supply() {
    let source = Inventory::new(&[(Material::Water, 1e24)]).unwrap();
    let mut budget =
        Budget::new(source, source, Inventory::default(), Inventory::default()).unwrap();
    let before = budget;
    assert_eq!(
        budget.transfer(Reservoir::Interior, Reservoir::Gas, Material::Water, 1.0),
        Err(CompositionError::TransferPrecision)
    );
    assert_eq!(budget, before);
    budget
        .transfer(Reservoir::Interior, Reservoir::Gas, Material::Water, 0.0)
        .unwrap();
    assert_eq!(budget, before);
}
