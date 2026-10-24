//! Accumulated simulation time, independent of rendering and frame rate.

#[derive(Debug)]
pub struct SimulationClock {
    pub seconds: f64,
    pub rate: f64,
    pub paused: bool,
}

impl Default for SimulationClock {
    fn default() -> Self {
        Self {
            seconds: 0.0,
            rate: 86_400.0,
            paused: false,
        }
    }
}

impl SimulationClock {
    pub fn advance(&mut self, real_seconds: f64) {
        if !self.paused {
            self.seconds += real_seconds * self.rate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_changes_and_pause_do_not_teleport() {
        let mut clock = SimulationClock::default();
        clock.advance(1.0);
        clock.rate = 1_000_000.0;
        assert_eq!(clock.seconds, 86400.0);
        clock.paused = true;
        clock.advance(10.0);
        assert_eq!(clock.seconds, 86400.0);
        clock.paused = false;
        clock.advance(1.0);
        assert_eq!(clock.seconds, 1_086_400.0);
    }
}
