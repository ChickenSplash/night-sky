//! Galactic stars: streaming them around the camera (`stream`), batching
//! them for upload (`starfield`) and drawing them (`render`, and natively
//! `points`).

#[cfg(not(target_arch = "wasm32"))]
pub mod points;
pub mod render;
pub mod starfield;
pub mod stream;
