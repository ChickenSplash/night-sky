//! Wrapping a route round bodies in the way, like a string pulled tight from
//! the ship to the target. Each body has a keep-clear sphere; the pilot
//! steers for the edge of the first one on the line, on the shorter side,
//! and works it out again every step, so moving bodies and several in a row
//! need nothing more.
use glam::DVec3;

/// A body's keep-clear sphere at one instant.
#[derive(Clone, Debug, PartialEq)]
pub struct Obstacle {
    pub id: String,
    pub centre: DVec3,
    pub velocity: DVec3,
    pub clear_m: f64,
}

impl Obstacle {
    /// Its keep-clear radius as seen from `from`: a ship already inside
    /// keeps clear of the sphere it is on, so it climbs out rather than in.
    fn radius_from(&self, from: DVec3) -> f64 {
        self.clear_m.min(self.centre.distance(from))
    }
}

/// Where to steer this step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub direction: DVec3,
    /// The obstacle whose edge is steered for, by index.
    pub around: Option<usize>,
}

/// A body met on the wrapped path, and how far the path turns round it.
#[derive(Clone, Debug, PartialEq)]
pub struct Bend {
    pub id: String,
    pub turn: f64,
}

/// The nearest obstacle the segment from `from`, `length` metres along
/// `along`, passes inside.
fn first_in_way(obstacles: &[Obstacle], from: DVec3, along: DVec3, length: f64) -> Option<usize> {
    obstacles
        .iter()
        .enumerate()
        .filter_map(|(i, obstacle)| {
            let offset = obstacle.centre - from;
            let ahead = offset.dot(along);
            let radius = obstacle.radius_from(from);
            let closest = offset - along * ahead.clamp(0.0, length.max(0.0));
            (ahead > 0.0 && closest.length_squared() < radius * radius * (1.0 - 1e-9))
                .then_some((i, ahead))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The direction from `from` that grazes the obstacle's sphere on the side
/// `toward` lies, and how far along it the sphere is touched.
fn edge(obstacle: &Obstacle, from: DVec3, toward: DVec3) -> (DVec3, f64) {
    let offset = obstacle.centre - from;
    let distance = offset.length();
    let centre = offset / distance;
    let side = (toward - centre * toward.dot(centre))
        .try_normalize()
        .unwrap_or_else(|| centre.any_orthonormal_vector());
    let sin = (obstacle.radius_from(from) / distance).min(1.0);
    let cos = (1.0 - sin * sin).sqrt();
    (centre * cos + side * sin, distance * cos)
}

/// Steer from `from` towards `goal`, stopping `standoff` short of it: along
/// the line when it is clear, else for the edge of the first sphere in the
/// way, checked in turn against the others.
pub fn aim(obstacles: &[Obstacle], from: DVec3, goal: DVec3, standoff: f64) -> Aim {
    let offset = goal - from;
    let mut direction = offset.normalize_or_zero();
    let mut length = offset.length() - standoff;
    let mut around = None;
    for _ in 0..obstacles.len() {
        let Some(i) = first_in_way(obstacles, from, direction, length) else {
            break;
        };
        (direction, length) = edge(&obstacles[i], from, direction);
        around = Some(i);
    }
    Aim { direction, around }
}

/// The wrapped path's length from `from` to the goal's centre, with the
/// bends on it, round obstacles frozen where they are now.
pub fn path(obstacles: &[Obstacle], from: DVec3, goal: DVec3, standoff: f64) -> (f64, Vec<Bend>) {
    let (mut at, mut length, mut bends) = (from, 0.0, Vec::new());
    for _ in 0..obstacles.len() {
        let offset = goal - at;
        let sight = offset.normalize_or_zero();
        let Some(i) = first_in_way(obstacles, at, sight, offset.length() - standoff) else {
            break;
        };
        let obstacle = &obstacles[i];
        let radius = obstacle.radius_from(at);
        let (direction, reach) = edge(obstacle, at, sight);
        let touch = (at + direction * reach - obstacle.centre).normalize();
        // Round the sphere until the goal clears its horizon.
        let out = goal - obstacle.centre;
        let horizon = (radius / out.length()).min(1.0).acos();
        let arc = (touch.angle_between(out) - horizon).max(0.0);
        let leave = rotate_towards(touch, out.normalize(), arc);
        length += reach + radius * arc;
        // Round a circle, the heading turns as far as the arc.
        bends.push(Bend {
            id: obstacle.id.clone(),
            turn: arc,
        });
        // Just outside, so the sphere just left is not met again.
        at = obstacle.centre + leave * radius * (1.0 + 1e-6);
    }
    (length + at.distance(goal), bends)
}

/// `from` turned `angle` radians towards `to`, both unit vectors.
fn rotate_towards(from: DVec3, to: DVec3, angle: f64) -> DVec3 {
    let side = (to - from * to.dot(from))
        .try_normalize()
        .unwrap_or_else(|| from.any_orthonormal_vector());
    from * angle.cos() + side * angle.sin()
}

/// Metres to the nearest sphere ahead along `heading`, among those the
/// ship is outside of; the one it may be skirting from inside is left to
/// the bend.
pub fn impact(obstacles: &[Obstacle], from: DVec3, heading: DVec3) -> Option<f64> {
    obstacles
        .iter()
        .filter_map(|obstacle| {
            let offset = obstacle.centre - from;
            let ahead = offset.dot(heading);
            let miss = obstacle.clear_m.powi(2) - (offset.length_squared() - ahead * ahead);
            (offset.length() > obstacle.clear_m && ahead > 0.0 && miss > 0.0)
                .then(|| ahead - miss.sqrt())
        })
        .min_by(f64::total_cmp)
}

/// How fast `aim`'s direction turns, in rad/s as an axis, for a ship moving
/// at `velocity` with the obstacle it skirts: the line to the obstacle's
/// centre turns, and the edge angle widens as the ship closes.
pub fn edge_turn(obstacle: &Obstacle, from: DVec3, velocity: DVec3, direction: DVec3) -> DVec3 {
    let offset = obstacle.centre - from;
    let distance = offset.length();
    let centre = offset / distance;
    let relative = obstacle.velocity - velocity;
    let mut turn = centre.cross(relative) / distance;
    if obstacle.clear_m < distance {
        // The edge angle is asin(clear ÷ distance).
        let cos = (1.0 - (obstacle.clear_m / distance).powi(2)).sqrt();
        let growth = centre.dot(relative);
        let widening = -obstacle.clear_m * growth / (distance * distance * cos.max(1e-3));
        turn += centre.cross(direction).normalize_or_zero() * widening;
    }
    turn
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(x: f64, clear_m: f64) -> Obstacle {
        Obstacle {
            id: format!("b{x}"),
            centre: DVec3::new(x, 0.0, 0.0),
            velocity: DVec3::ZERO,
            clear_m,
        }
    }

    #[test]
    fn a_clear_line_is_flown_straight() {
        let obstacles = [ball(5.0, 1.0)];
        let goal = DVec3::new(10.0, 3.0, 0.0);
        let aim = aim(&obstacles, DVec3::ZERO, goal, 0.1);
        assert_eq!(aim.around, None);
        assert!((aim.direction - goal.normalize()).length() < 1e-12);
        let (length, bends) = path(&obstacles, DVec3::ZERO, goal, 0.1);
        assert!((length - goal.length()).abs() < 1e-9);
        assert!(bends.is_empty());
    }

    #[test]
    fn a_body_in_the_way_is_grazed_on_the_shorter_side() {
        let obstacles = [ball(5.0, 1.0)];
        let goal = DVec3::new(10.0, 0.1, 0.0);
        let aim = aim(&obstacles, DVec3::ZERO, goal, 0.1);
        assert_eq!(aim.around, Some(0));
        // Tangent: asin(1 ÷ 5) off the line, towards the goal's side.
        assert!((aim.direction.angle_between(DVec3::X) - 0.2f64.asin()).abs() < 1e-9);
        assert!(aim.direction.y > 0.0);
        // The goal stopping short of the sphere is not wrapped round.
        let short = super::aim(&obstacles, DVec3::ZERO, DVec3::new(10.0, 0.1, 0.0), 6.5);
        assert_eq!(short.around, None);
    }

    #[test]
    fn the_wrapped_path_is_the_tight_string() {
        // Straight through the middle: two tangents and the arc between.
        let obstacles = [ball(5.0, 1.0)];
        let goal = DVec3::new(10.0, 0.0, 0.0);
        let (length, bends) = path(&obstacles, DVec3::ZERO, goal, 0.0);
        let tangent = 24f64.sqrt();
        let arc = std::f64::consts::PI - 2.0 * 0.2f64.acos();
        assert!((length - (2.0 * tangent + arc)).abs() < 1e-6, "{length}");
        assert_eq!(bends.len(), 1);
        assert!((bends[0].turn - arc).abs() < 1e-6, "{bends:?}");
    }

    #[test]
    fn a_ship_inside_a_sphere_climbs_out_along_its_horizon() {
        let obstacles = [ball(0.0, 2.0)];
        let from = DVec3::new(-1.0, 0.0, 0.0);
        let aim = aim(&obstacles, from, DVec3::new(10.0, 0.5, 0.0), 0.0);
        assert_eq!(aim.around, Some(0));
        assert!(aim.direction.dot(DVec3::X).abs() < 1e-9);
        assert!(aim.direction.y > 0.0);
        // Heading out, nothing is in the way.
        let out = super::aim(&obstacles, from, DVec3::new(-10.0, 0.0, 0.0), 0.0);
        assert_eq!(out.around, None);
    }

    #[test]
    fn two_bodies_in_a_row_are_both_cleared() {
        let obstacles = [ball(5.0, 1.0), ball(3.0, 1.5)];
        let goal = DVec3::new(10.0, 0.2, 0.0);
        let aim = aim(&obstacles, DVec3::ZERO, goal, 0.0);
        for obstacle in &obstacles {
            let offset = obstacle.centre;
            let closest = offset - aim.direction * offset.dot(aim.direction);
            assert!(closest.length() >= obstacle.clear_m - 1e-9);
        }
        let (_, bends) = path(&obstacles, DVec3::ZERO, goal, 0.0);
        assert!(!bends.is_empty());
    }

    #[test]
    fn impact_is_measured_to_the_nearest_sphere_ahead() {
        let obstacles = [ball(5.0, 1.0), ball(9.0, 1.0), ball(0.0, 0.5)];
        let hit = impact(&obstacles, DVec3::ZERO, DVec3::X).unwrap();
        assert!((hit - 4.0).abs() < 1e-12);
        assert_eq!(impact(&obstacles, DVec3::ZERO, DVec3::Y), None);
    }

    #[test]
    fn the_edge_holds_still_along_its_own_tangent() {
        let obstacle = ball(5.0, 1.0);
        let from = DVec3::ZERO;
        let (direction, _) = edge(&obstacle, from, DVec3::Y);
        let turn = edge_turn(&obstacle, from, direction * 3.0, direction);
        assert!(turn.length() < 1e-9, "{turn}");
    }
}
