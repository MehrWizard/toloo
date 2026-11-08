//! Toloo Protocol v0.2 — portable protocol core.
//!
//! No I/O and no network runtime: envelopes, canonical JSON, cryptography,
//! identities, rules, events, discovery and private messages. This crate is
//! shared by the native runtime (`toloo-lib`) and the browser bindings
//! (`toloo-wasm`).

pub mod avatar;
pub mod base64url;
pub mod canonical;
pub mod crypto;
pub mod discovery;
pub mod envelope;
pub mod events;
pub mod hex;
pub mod ids;
pub mod membership;
pub mod memzero;
pub mod pow;
pub mod private;
pub mod rules;
pub mod types;
pub mod vectors;

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
