use super::*;
use space_simulation::galaxy::population::VISIBLE_MAGNITUDE;

#[test]
fn batches_only_upload_stars_near_enough_to_see() {
    let eye = DVec3::new(8200.0, 20.0, 0.0);
    let point = |distance: f64| StarPoint {
        position_pc: [8200.0 + distance, 20.0, 0.0],
        luminosity_solar: 1.0,
        temperature_k: 5772.0,
        radius_solar: 1.0,
        separation_au: 0.0,
        component: space_simulation::galaxy::Component::ThinDisc,
        index: 0,
    };
    let mut stars = Vec::new();
    pack(
        &mut stars,
        &[point(10.0), point(100.0)],
        [8200.0, 0.0, 0.0],
        eye,
        VISIBLE_MAGNITUDE,
    );
    assert_eq!(stars.len(), 1);
    assert_eq!(stars[0].position, [10.0, 20.0, 0.0]);
    stars.clear();
    pack(
        &mut stars,
        &[point(100.0)],
        [8200.0, 0.0, 0.0],
        eye,
        VISIBLE_MAGNITUDE,
    );
    assert!(stars.is_empty());
}
