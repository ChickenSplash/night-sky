//! Placeholder wireframe in body-local metres: cuboid, -Z pyramid nose,
//! and triangular wings rooted on the bottom (-Y) edge. No model assets.
const HALF: f64 = super::SHIP_SIZE_M / 2.0;
pub const HULL_VERTICES: [[f64; 3]; 11] = [
    [-4.0, -3.0, -9.0],
    [4.0, -3.0, -9.0],
    [-4.0, 3.0, -9.0],
    [4.0, 3.0, -9.0],
    [-4.0, -3.0, HALF],
    [4.0, -3.0, HALF],
    [-4.0, 3.0, HALF],
    [4.0, 3.0, HALF],
    [0.0, 0.0, -HALF],
    [-12.0, -3.0, 9.0],
    [12.0, -3.0, 9.0],
];

pub const HULL_EDGES: [(usize, usize); 20] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
    (0, 8),
    (1, 8),
    (2, 8),
    (3, 8),
    (0, 9),
    (9, 4),
    (1, 10),
    (10, 5),
];
