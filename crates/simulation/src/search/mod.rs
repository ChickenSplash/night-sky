//! Object search: one query model and engine for every front end (the
//! in-game browser, the `ns` CLI, and later a query builder and the ship's
//! navigator). See `docs/object-search-plan.md`.
mod engine;
pub mod query;
pub mod registry;
mod runner;
#[cfg(test)]
mod tests;
#[cfg(not(target_arch = "wasm32"))]
mod worker;

pub use engine::{
    DEFAULT_WORK_CAP, Origin, PC_PER_LY, Progress, Search, SearchRequest, SearchResult, State,
};
pub use query::{Filter, Query, Scope, Sort, Test};
pub use runner::{Order, Runner, SearchUpdate};
#[cfg(not(target_arch = "wasm32"))]
pub use worker::SearchService;
