use super::*;

#[test]
fn region_transport_keeps_minor_constituents_not_just_the_dominant_material() {
    let inventory = Inventory::new(&[
        (Material::Hydrogen, 100.0),
        (Material::Nitrogen, 0.001),
        (Material::CarbonDioxide, 0.0001),
        (Material::Methane, 1e-12),
    ])
    .unwrap();
    let sample = dry::Sample {
        radius_m: 1.0,
        enclosed_mass_kg: 100.0,
        pressure_pa: 1.0,
        density_kg_m3: 1.0,
    };
    let layer = region(&inventory, sample, sample, None, "envelope", "fixture");
    assert_eq!(layer.materials.len(), 4);
    for material in Material::ALL {
        let actual = layer
            .materials
            .iter()
            .find(|m| m.material == material.id())
            .map_or(0.0, |m| m.mass_kg);
        assert_eq!(actual, inventory.mass_kg(material));
    }
}
