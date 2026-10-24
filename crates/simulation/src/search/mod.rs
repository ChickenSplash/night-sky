//! Object search: one query model and engine for every front end (the
//! in-game browser, the `ns` CLI, and later a query builder and the ship's
//! navigator). See `docs/simulation/search.md`.
pub(crate) mod engine;
pub mod live;
#[cfg(test)]
mod probe;
pub mod query;
pub mod registry;
mod runner;
#[cfg(test)]
mod tests;
mod worker;

pub use engine::{
    DEFAULT_WORK_CAP, Origin, PC_PER_LY, Progress, Search, SearchRequest, SearchResult, State,
};
pub use query::{Filter, Query, Scope, Sort, Test};
pub use runner::SearchUpdate;
pub use worker::SearchService;
