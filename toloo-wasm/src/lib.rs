//! WebAssembly bindings for the Toloo protocol core.
//!
//! Everything protocol-level lives in `toloo-core`, which is the same crate
//! the native runtime uses. This crate only exposes a JSON-in/JSON-out
//! contract to JavaScript plus a thin `wasm-bindgen` layer.

pub mod contract;

#[cfg(target_arch = "wasm32")]
mod bindings;

#[cfg(target_arch = "wasm32")]
pub use bindings::*;
