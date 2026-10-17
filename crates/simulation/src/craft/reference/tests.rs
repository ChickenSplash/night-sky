use super::*;
use crate::AU_METRES;
use space_shared::{Barycentre, Orbit, Star};

fn star(letter: &str, mass_solar: f64) -> Star {
    Star {
        id: format!("t#{letter}"),
        page: 0,
        spectral: "G2 V".into(),
        kind: "Yellow dwarf".into(),
        mass_solar,
        radius_solar: 1.0,
        luminosity_solar: 1.0,
        temperature_k: 5_772.0,
        colour: [255; 3],
        spin: Default::default(),
        component: letter.into(),
    }
}

fn pair(letters: &str, members: [&str; 2], semi_major_au: f64) -> Barycentre {
    Barycentre {
        id: format!("t#{letters}"),
        page: 0,
        component: letters.into(),
        members: members.map(|m| format!("t#{m}")),
        orbit: Orbit {
            semi_major_au,
            ..Orbit::default()
        },
    }
}

/// A hierarchical triple of Sun-like stars, (AB)C: A and B 20 AU apart,
/// C 400 AU from their barycentre, with an Earth at 1 AU around A. At time
/// zero every orbit lies along +X: AB at -133 AU, C at +267 AU, and A and B
/// 10 AU either side of AB.
fn triple() -> Field {
    let planet = Planet {
        id: "t#A/0".into(),
        host: "t#A".into(),
        orbit: Orbit {
            semi_major_au: 1.0,
            ..Orbit::default()
        },
        radius_km: 6_371.0,
        mass_earth: 1.0,
        ..Planet::default()
    };
    let system = StarSystem {
        id: "t".into(),
        name: "Test".into(),
        position_ly: [0.0; 3],
        planets: vec![planet],
        stars: vec![star("A", 1.0), star("B", 1.0), star("C", 1.0)],
        barycentres: vec![
            pair("AB", ["A", "B"], 20.0),
            pair("ABC", ["AB", "C"], 400.0),
        ],
        page_stars: Vec::new(),
    };
    Field::new(system, 0)
}

fn at(field: &Field, id: &str, offset_au: [f64; 3]) -> [f64; 3] {
    let centre = field.reference_by_id(id, 0.0).unwrap().state.position;
    std::array::from_fn(|i| centre[i] + offset_au[i] * AU_METRES)
}

fn chosen(field: &Field, current: Option<&str>, position: [f64; 3]) -> String {
    field.select_reference(current, position, 0.0).unwrap().id
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn the_reference_walks_down_the_stellar_hierarchy() {
    let field = triple();
    let ab = at(&field, "t#AB", [0.0; 3]);
    assert!((ab[0] / AU_METRES + 400.0 / 3.0).abs() < 1e-9, "{ab:?}");
    // Circumstellar, around one star of the close pair.
    assert_eq!(
        chosen(&field, None, at(&field, "t#A", [0.0, 0.0, 2.0])),
        "t#A"
    );
    // Circumbinary: outside both members' zones, inside the pair's.
    let around_pair = field
        .select_reference(None, at(&field, "t#AB", [0.0, 0.0, 30.0]), 0.0)
        .unwrap();
    assert_eq!(around_pair.id, "t#AB");
    assert_eq!(around_pair.kind, ReferenceKind::Barycentre);
    assert_eq!(around_pair.radius_m, 0.0);
    assert!((around_pair.gm / (2.0 * SUN_GM) - 1.0).abs() < 1e-12);
    // The distant third, alone.
    assert_eq!(
        chosen(&field, None, at(&field, "t#C", [0.0, 0.0, 20.0])),
        "t#C"
    );
    // Far outside everything, the whole system.
    let far = field.select_reference(None, [0.0, 0.0, 3_000.0 * AU_METRES], 0.0);
    let far = far.unwrap();
    assert_eq!(far.id, "t#ABC");
    assert_eq!(far.reach_m, f64::INFINITY);
    assert!((far.gm / (3.0 * SUN_GM) - 1.0).abs() < 1e-12);
    // Inside a planet's Hill sphere, the planet, with the velocity it orbits at.
    let planet = field
        .select_reference(None, at(&field, "t#A/0", [1e-4, 0.0, 0.0]), 0.0)
        .unwrap();
    assert_eq!(planet.kind, ReferenceKind::Planet);
    let host = field.reference_by_id("t#A", 0.0).unwrap().state.velocity;
    let speed = distance(planet.state.velocity, host);
    assert!((speed / 29_785.0 - 1.0).abs() < 1e-3, "{speed}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_reference_is_kept_past_its_boundary_by_the_margin() {
    let field = triple();
    let zone = field.reference_by_id("t#A", 0.0).unwrap().reach_m / AU_METRES;
    // Holman & Wiegert for an equal-mass circular pair, 20 AU apart.
    assert!((zone - (0.464 - 0.380 * 0.5) * 20.0).abs() < 1e-9, "{zone}");
    let just_out = at(&field, "t#A", [0.0, 0.0, zone * 1.05]);
    assert_eq!(chosen(&field, None, just_out), "t#AB");
    assert_eq!(chosen(&field, Some("t#A"), just_out), "t#A");
    // Held through a planet inside it, too.
    assert_eq!(chosen(&field, Some("t#A/0"), just_out), "t#A");
    // Past the margin it switches out, and back in only inside the zone.
    let beyond = at(&field, "t#A", [0.0, 0.0, zone * 1.15]);
    assert_eq!(chosen(&field, Some("t#A"), beyond), "t#AB");
    assert_eq!(chosen(&field, Some("t#AB"), just_out), "t#AB");

    let hill = field.reference_by_id("t#A/0", 0.0).unwrap().reach_m / AU_METRES;
    let edge = at(&field, "t#A/0", [hill * 1.05, 0.0, 0.0]);
    assert_eq!(chosen(&field, None, edge), "t#A");
    assert_eq!(chosen(&field, Some("t#A/0"), edge), "t#A/0");
    let gone = at(&field, "t#A/0", [hill * 1.15, 0.0, 0.0]);
    assert_eq!(chosen(&field, Some("t#A/0"), gone), "t#A");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_craft_keeps_its_reference_and_points_relative_to_it() {
    let field = triple();
    let star = field.reference_by_id("t#A", 0.0).unwrap();
    // On the edge of A's zone, where with no history the pair would rule.
    let zone = star.reach_m;
    let offset = [0.0, 0.0, zone * 1.05];
    let state = State {
        position: std::array::from_fn(|i| star.state.position[i] + offset[i]),
        velocity: star.state.velocity,
    };
    let mut craft = Craft::new("c".into(), "C".into(), CraftKind::Ship, field_frame(), 0);
    craft.place(state, 0);
    craft.reference = Some("t#A".into());
    craft.point(PointTarget::RadialOut, None).unwrap();
    // Past a reference choice, a second in.
    craft.advance(&field, 1.2, usize::MAX);
    assert_eq!(craft.reference.as_deref(), Some("t#A"));
    // Radial out from A, not from the pair's barycentre 10 AU away.
    let direction = PointTarget::RadialOut
        .direction(
            craft.state(),
            craft.seconds(),
            &field,
            craft.reference.as_deref(),
        )
        .unwrap();
    assert!(direction.z > 0.999, "{direction:?}");
    // A saved craft keeps it.
    let copy: Craft = serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
    assert_eq!(copy.reference, craft.reference);
}

fn field_frame() -> Frame {
    Frame::System { star: "t".into() }
}
