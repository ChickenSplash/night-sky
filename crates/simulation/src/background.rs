//! Shared CPU budget for visible-star streaming and experimental planet solves.
//! Permits limit active work, not idle OS threads. No main-thread waits.
use std::sync::{Arc, Condvar, Mutex, OnceLock};

pub fn threads() -> usize {
    (std::thread::available_parallelism().map_or(1, |n| n.get()) * 3 / 4).max(1)
}

pub fn shared() -> Arc<Budget> {
    static SHARED: OnceLock<Arc<Budget>> = OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(Budget::new(threads())))
        .clone()
}

#[derive(Default)]
struct State {
    used: usize,
    stars: usize,
    waiting: usize,
}

pub struct Budget {
    limit: usize,
    state: Mutex<State>,
    changed: Condvar,
}

impl Budget {
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            state: Mutex::default(),
            changed: Condvar::new(),
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Background stream coordinator only. Blocking here never blocks a frame.
    pub fn star(self: &Arc<Self>) -> Permit {
        let mut s = self.state.lock().unwrap();
        s.waiting += 1;
        while s.used == self.limit {
            s = self.changed.wait(s).unwrap();
        }
        s.waiting -= 1;
        s.used += 1;
        s.stars += 1;
        Permit {
            budget: self.clone(),
            star: true,
        }
    }

    /// Extra star workers and grids never wait while holding a coordinator slot.
    pub fn try_star(self: &Arc<Self>) -> Option<Permit> {
        let mut s = self.state.lock().unwrap();
        if s.used == self.limit {
            return None;
        }
        s.used += 1;
        s.stars += 1;
        Some(Permit {
            budget: self.clone(),
            star: true,
        })
    }

    /// Stars win at admission. Reserve one slot for new star work on multi-core
    /// machines: running numerical solves are bounded but not preemptible.
    pub fn try_planet(self: &Arc<Self>) -> Option<Permit> {
        let mut s = self.state.lock().unwrap();
        let ceiling = self.limit.saturating_sub(1).max(1);
        if s.used >= ceiling || s.stars > 0 || s.waiting > 0 {
            return None;
        }
        s.used += 1;
        Some(Permit {
            budget: self.clone(),
            star: false,
        })
    }
}

pub struct Permit {
    budget: Arc<Budget>,
    star: bool,
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut s = self.budget.state.lock().unwrap();
        s.used -= 1;
        s.stars -= usize::from(self.star);
        self.budget.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shares_the_cap_and_reserves_star_headroom() {
        let b = Arc::new(Budget::new(4));
        let p: Vec<_> = (0..3).map(|_| b.try_planet().unwrap()).collect();
        assert!(b.try_planet().is_none());
        let star = b.star();
        assert!(b.try_star().is_none());
        drop(p);
        assert!(b.try_planet().is_none());
        let extra = b.try_star().unwrap();
        drop(star);
        assert!(b.try_planet().is_none());
        drop(extra);
        assert!(b.try_planet().is_some());
    }
    #[test]
    fn single_thread_still_progresses() {
        let b = Arc::new(Budget::new(1));
        let p = b.try_planet().unwrap();
        assert!(b.try_star().is_none());
        drop(p);
        let s = b.star();
        assert!(b.try_planet().is_none());
        drop(s);
        assert!(b.try_planet().is_some());
    }
}
