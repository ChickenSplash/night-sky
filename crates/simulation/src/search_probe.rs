//! What object searches cost, measured on the engine itself. Run with
//! `cargo test -p space-simulation --release search_probe -- --ignored --nocapture`
//! (`PROBE_SECONDS` caps each case, default 60; `PROBE_THREADS` sets the
//! threads of the multi-threaded runs, default all cores).

use crate::galaxy::GalaxyModel;
use crate::search::{Origin, PC_PER_LY, Query, Search, SearchRequest, State};
use std::time::{Duration, Instant};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "measurement, minutes in release"]
fn search_probe() {
    let home = GalaxyModel::milky_way().home_pc;
    let limit = Duration::from_secs(env("PROBE_SECONDS", 60));
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as u64;
    let threads = env("PROBE_THREADS", cores) as usize;
    let centre_500 = [500.0 * PC_PER_LY, 0.0, 0.0];
    // Nothing passes the first filter: the whole sphere is searched.
    let nothing = "kind=star temperature_k=..0";
    let typical = "kind=planet planet_class=sub_neptune";
    let cases = [
        ("home", home, 50.0, nothing),
        ("home", home, 50.0, typical),
        ("home", home, 1_000.0, nothing),
        ("home", home, 1_000.0, typical),
        ("500 ly out", centre_500, 50.0, typical),
        ("centre", [0.0; 3], 5.0, nothing),
        ("centre", [0.0; 3], 50.0, typical),
    ];
    println!(
        "| where | radius ly | query | threads | time s | state | to ly | work M | cells | stars M | rejected early M | evaluated | results | cache MB |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (name, centre, radius, filter) in cases {
        for threads in [1, threads] {
            let query = Query::parse(&format!("{filter} within_ly={radius}")).unwrap();
            let mut search = Search::new(SearchRequest {
                id: 0,
                query,
                origin: Origin::Position { pc: centre },
                work_cap: u64::MAX,
            })
            .unwrap();
            let start = Instant::now();
            let mut peak_cache = 0;
            let progress = loop {
                let progress = search.step(1_000_000, threads);
                peak_cache = peak_cache.max(progress.cache_bytes);
                if progress.state != State::Running || start.elapsed() > limit {
                    break progress;
                }
            };
            println!(
                "| {name} | {radius} | {} | {threads} | {:.2} | {:?} | {:.1} | {:.0} | {} | {:.1} | {:.1} | {} | {} | {:.0} |",
                if filter == nothing {
                    "nothing"
                } else {
                    "sub-Neptunes"
                },
                start.elapsed().as_secs_f64(),
                progress.state,
                progress.complete_to_ly,
                progress.work as f64 / 1e6,
                progress.cells,
                progress.stars as f64 / 1e6,
                progress.rejected_early as f64 / 1e6,
                progress.evaluated,
                search.results().len(),
                peak_cache as f64 / 1e6,
            );
        }
    }
}
