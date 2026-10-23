//! Transport for the shared bounded voyage loader: its warm worker pool, which
//! never generates on the frame update or waits for a result.
pub use space_simulation::craft::voyage::loading::Native as Transport;
