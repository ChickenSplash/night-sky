//! Shared CPU budget for visible stars, object search and planet refinement.
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
    searches: usize,
    search_demand: usize,
    search_turn: bool,
}

pub struct Budget {
    limit: usize,
    state: Mutex<State>,
    changed: Condvar,
}

/// Instantaneous admitted worker slots, not sampled CPU utilisation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activity {
    pub stars: usize,
    pub search: usize,
    pub planets: usize,
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

    pub fn activity(&self) -> Activity {
        let s = self.state.lock().unwrap();
        Activity {
            stars: s.stars,
            search: s.searches,
            planets: s.used - s.stars - s.searches,
        }
    }

    /// Background stream coordinator only. Blocking here never blocks a frame.
    pub fn star(self: &Arc<Self>) -> Permit {
        let mut s = self.state.lock().unwrap();
        s.waiting += 1;
        while !self.star_available(&s) {
            s = self.changed.wait(s).unwrap();
        }
        s.waiting -= 1;
        s.used += 1;
        s.stars += 1;
        s.search_turn = true;
        Permit {
            budget: self.clone(),
            kind: Kind::Star,
        }
    }

    /// Extra star workers and grids never wait while holding a coordinator slot.
    pub fn try_star(self: &Arc<Self>) -> Option<Permit> {
        let mut s = self.state.lock().unwrap();
        if !self.star_available(&s) {
            return None;
        }
        s.used += 1;
        s.stars += 1;
        s.search_turn = true;
        Some(Permit {
            budget: self.clone(),
            kind: Kind::Star,
        })
    }

    fn star_available(&self, s: &State) -> bool {
        let reserve = s.search_demand > 0 && s.searches == 0 && (self.limit > 1 || s.search_turn);
        s.used < self.limit - usize::from(reserve)
    }

    /// Keep one admission available for an active search service. On a one-slot
    /// budget stars and search alternate; no frame thread waits for either.
    pub fn search_demand(self: &Arc<Self>) -> SearchDemand {
        self.state.lock().unwrap().search_demand += 1;
        SearchDemand {
            budget: self.clone(),
        }
    }

    /// The search coordinator's guaranteed share, acquired without blocking its
    /// command inbox. Existing work must finish before this can be granted.
    pub fn try_search(self: &Arc<Self>) -> Option<Permit> {
        let mut s = self.state.lock().unwrap();
        if s.used == self.limit
            || s.searches > 0
            || (self.limit == 1 && s.waiting > 0 && !s.search_turn)
        {
            return None;
        }
        s.used += 1;
        s.searches += 1;
        s.search_turn = false;
        Some(Permit {
            budget: self.clone(),
            kind: Kind::Search,
        })
    }

    /// Extra search capacity is opportunistic: active/waiting stars win, and
    /// one slot remains free for newly arriving star work on multi-core budgets.
    pub fn try_search_extra(self: &Arc<Self>) -> Option<Permit> {
        let mut s = self.state.lock().unwrap();
        if s.searches == 0 || s.stars > 0 || s.waiting > 0 || s.used >= self.limit.saturating_sub(1)
        {
            return None;
        }
        s.used += 1;
        s.searches += 1;
        Some(Permit {
            budget: self.clone(),
            kind: Kind::Search,
        })
    }

    /// Stars win at admission. Reserve one slot for new star work on multi-core
    /// machines: running numerical solves are bounded but not preemptible.
    pub fn try_planet(self: &Arc<Self>) -> Option<Permit> {
        let mut s = self.state.lock().unwrap();
        let ceiling = self.limit.saturating_sub(1).max(1);
        if s.used >= ceiling
            || s.stars > 0
            || s.waiting > 0
            || s.search_demand > 0
            || s.searches > 0
        {
            return None;
        }
        s.used += 1;
        Some(Permit {
            budget: self.clone(),
            kind: Kind::Planet,
        })
    }
}

pub struct Permit {
    budget: Arc<Budget>,
    kind: Kind,
}

enum Kind {
    Star,
    Search,
    Planet,
}

pub struct SearchDemand {
    budget: Arc<Budget>,
}

impl Drop for SearchDemand {
    fn drop(&mut self) {
        self.budget.state.lock().unwrap().search_demand -= 1;
        self.budget.changed.notify_all();
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut s = self.budget.state.lock().unwrap();
        s.used -= 1;
        match self.kind {
            Kind::Star => s.stars -= 1,
            Kind::Search => s.searches -= 1,
            Kind::Planet => {}
        }
        self.budget.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_search_keeps_one_slot_while_stars_take_the_rest() {
        let b = Arc::new(Budget::new(4));
        let demand = b.search_demand();
        let stars: Vec<_> = (0..3).map(|_| b.try_star().unwrap()).collect();
        assert!(b.try_star().is_none());
        let search = b.try_search().unwrap();
        assert_eq!(
            b.activity(),
            Activity {
                stars: 3,
                search: 1,
                planets: 0
            }
        );
        assert!(b.try_search_extra().is_none());
        assert!(b.try_planet().is_none());
        drop(search);
        assert!(b.try_star().is_none());
        let search = b.try_search().unwrap();
        drop(stars);
        let extras: Vec<_> = (0..2).map(|_| b.try_search_extra().unwrap()).collect();
        assert!(b.try_search_extra().is_none());
        let star = b.try_star().unwrap();
        assert_eq!(
            b.activity(),
            Activity {
                stars: 1,
                search: 3,
                planets: 0
            }
        );
        drop((star, search, extras, demand));
        let planet = b.try_planet().unwrap();
        assert_eq!(
            b.activity(),
            Activity {
                stars: 0,
                search: 0,
                planets: 1
            }
        );
        drop(planet);
        assert_eq!(
            b.activity(),
            Activity {
                stars: 0,
                search: 0,
                planets: 0
            }
        );
    }

    #[test]
    fn search_demand_releases_reservation_and_blocks_new_refinement() {
        let b = Arc::new(Budget::new(2));
        let demand = b.search_demand();
        assert!(b.try_planet().is_none());
        let star = b.try_star().unwrap();
        assert!(b.try_star().is_none());
        drop(demand);
        assert!(b.try_star().is_some());
        drop(star);
        assert!(b.try_planet().is_some());
    }

    #[test]
    fn single_slot_alternates_search_with_waiting_stars() {
        let b = Arc::new(Budget::new(1));
        let _demand = b.search_demand();
        let star = b.try_star().unwrap();
        drop(star);
        assert!(b.try_star().is_none());
        let search = b.try_search().unwrap();
        assert!(b.try_star().is_none());
        drop(search);
        // Simulate a star coordinator already waiting for admission.
        b.state.lock().unwrap().waiting = 1;
        assert!(b.try_search().is_none());
        let star = b.try_star().unwrap();
        drop(star);
        assert!(b.try_search().is_some());
        b.state.lock().unwrap().waiting = 0;
    }

    #[test]
    fn queued_stars_prevent_search_from_borrowing_extra_capacity() {
        let b = Arc::new(Budget::new(4));
        let _demand = b.search_demand();
        let _search = b.try_search().unwrap();
        b.state.lock().unwrap().waiting = 1;
        assert!(b.try_search_extra().is_none());
        b.state.lock().unwrap().waiting = 0;
        assert!(b.try_search_extra().is_some());
    }
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
