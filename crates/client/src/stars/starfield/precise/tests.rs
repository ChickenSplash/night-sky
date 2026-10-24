use super::super::hide;
use super::*;
use space_simulation::galaxy::PC_METRES;
use space_simulation::galaxy::population::VISIBLE_MAGNITUDE;

/// Repeats the GPU's f32 placement of a batched point: its f32 offset
/// from the block centre, scaled, plus the centre's camera-relative offset.
#[test]
fn batched_points_stay_within_a_quarter_pixel_outside_the_precise_radius() {
    let quarter_pixel = 0.25 * 2.0 * 25.0_f64.to_radians().tan() / 1000.0;
    let centre = DVec3::new(8192.0, 64.0, -64.0);
    for offset_pc in [0.5, 60.0, 110.0, 8_000.0, 110_000.0] {
        let star = centre + DVec3::new(0.577, -0.614, 0.538).normalize() * offset_pc;
        let away = DVec3::new(-0.3, 0.8, 0.52).normalize();
        let eye = star + away * PRECISE * offset_pc * 1.001;
        let exact_pc = star - eye;
        // From framing the star itself down to a planet-scale zoom.
        for zoom in [1.0, 1e-3, 1e-6] {
            let factor = 1000.0 / (exact_pc.length() * zoom);
            let vertex = (star - centre).as_vec3();
            let world = vertex * factor as f32 + ((centre - eye) * factor).as_vec3();
            let exact = exact_pc * factor;
            let angle = (world.as_dvec3() - exact).length() / exact.length();
            assert!(
                angle < quarter_pixel,
                "{offset_pc} pc, zoom {zoom}: {angle:e}"
            );
        }
    }
}

#[test]
fn stars_near_the_camera_become_precise_candidates() {
    let cell = Cell::containing(10, [8200.0, 20.0, 0.0]).unwrap();
    let centre = DVec3::from_array(cell.centre_pc());
    let point = |position: DVec3, index| StarPoint {
        position_pc: position.to_array(),
        luminosity_solar: 1.0,
        temperature_k: 5772.0,
        radius_solar: 1.0,
        separation_au: 0.0,
        component: space_simulation::galaxy::Component::ThinDisc,
        index,
    };
    let star = centre + DVec3::new(40.0, 0.0, 0.0);
    let mut field = Starfield::default();
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(
            cell,
            vec![point(star, 0), point(centre, 1)],
        ),
    );
    let magnitude = VISIBLE_MAGNITUDE;
    let near = candidates(&field, star + DVec3::new(0.0, 0.05, 0.0), magnitude);
    assert_eq!(near.len(), 1);
    assert!(near[0].is((cell, point(star, 0).component, 0)));
    assert!((near[0].offset_pc - 40.0).abs() < 1e-9);
    // Beyond the prepared radius and the rescan margin, nothing is gathered.
    let far = star + DVec3::new(0.0, PREPARE * PRECISE * 40.0 + RESCAN_PC + 0.01, 0.0);
    assert!(candidates(&field, far, magnitude).is_empty());
    // Nor a star too faint to show from anywhere within the rescan margin.
    let faint = StarPoint {
        luminosity_solar: 1e-8,
        ..point(star, 0)
    };
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(cell, vec![faint]),
    );
    let eye = star + DVec3::new(0.0, RESCAN_PC + 0.05, 0.0);
    assert!(candidates(&field, eye, magnitude).is_empty());
}

#[test]
fn only_the_visited_star_is_hidden_from_its_batch() {
    let cell = Cell::containing(10, [8200.0, 20.0, 0.0]).unwrap();
    let centre = cell.centre_pc();
    let star = |offset: f64, index| StarPoint {
        position_pc: [
            centre[0] + offset,
            centre[1] - offset / 3.0,
            centre[2] + 0.1,
        ],
        luminosity_solar: 1.0,
        temperature_k: 5772.0,
        radius_solar: 1.0,
        separation_au: 0.0,
        component: space_simulation::galaxy::Component::ThinDisc,
        index,
    };
    let (visited, neighbour) = (star(37.123_456_789, 0), star(37.2, 1));
    let mut field = Starfield::default();
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(cell, vec![visited, neighbour]),
    );
    let key = (cell, visited.component, 0);
    let at = star_position(&field, key).unwrap();
    assert_eq!(at, visited.position_pc);
    let hidden = hide(at, centre);
    // The shader compares exactly what the batch uploaded.
    assert_eq!(
        Vec3::from_array(instance(&visited, centre).position),
        hidden.truncate()
    );
    assert_ne!(
        Vec3::from_array(instance(&neighbour, centre).position),
        hidden.truncate()
    );
    assert!(hidden.w > 0.0);
    assert!(star_position(&field, (cell, visited.component, 9)).is_none());
}

#[test]
fn precise_sprite_is_ready_before_batch_culls_at_interstellar_range() {
    let cell = Cell::containing(6, [8200.0, 20.0, 0.0]).unwrap();
    let centre = DVec3::from_array(cell.centre_pc());
    // A bright star far from its large cell's centre can switch several
    // light years before the actual 0.02 ly system handoff.
    let position = centre + DVec3::X * 980.0;
    let point = StarPoint {
        position_pc: position.to_array(),
        luminosity_solar: 10_000.0,
        temperature_k: 10_000.0,
        radius_solar: 10.0,
        separation_au: 0.0,
        component: space_simulation::galaxy::Component::ThinDisc,
        index: 0,
    };
    let mut field = Starfield::default();
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(cell, vec![point]),
    );
    let eye = position + DVec3::Z * (6.0 * space_simulation::LY_METRES / PC_METRES);
    let candidate = candidates(&field, eye, VISIBLE_MAGNITUDE);
    assert_eq!(candidate.len(), 1);
    let radius = PRECISE * candidate[0].offset_pc;
    let distance = eye.distance(position);
    assert!(distance < PREPARE * radius);
    assert!(distance < radius, "sprite must be visible at 6 ly");
    assert!(
        distance > BATCH_HIDE * candidate[0].offset_pc,
        "batch must still cover the six-light-year transition"
    );
}
