use super::*;

#[test]
fn blocks_outside_the_view_are_culled() {
    // Looking down -Z with a 90° vertical field, twice as wide as high.
    let camera = GlobalTransform::IDENTITY;
    let sight = Sight::new(&camera, 1.0, UVec2::new(2000, 1000), 0.0);
    assert!(sight.sees(DVec3::new(0.0, 0.0, -100.0), 1.0));
    assert!(sight.sees(DVec3::new(190.0, 0.0, -100.0), 1.0));
    assert!(!sight.sees(DVec3::new(0.0, 0.0, 100.0), 1.0));
    assert!(!sight.sees(DVec3::new(0.0, 150.0, -100.0), 1.0));
    assert!(!sight.sees(DVec3::new(250.0, 0.0, -100.0), 1.0));
    // A block reaching into the view from outside it, or around the eye.
    assert!(sight.sees(DVec3::new(0.0, 150.0, -100.0), 40.0));
    assert!(sight.sees(DVec3::new(0.0, 0.0, 5.0), 10.0));
}
