//! Toloo Protocol v0.2 — stateful native runtime.
//!
//! The protocol core (envelopes, canonical JSON, crypto, identities, rules,
//! events, discovery, private messages) lives in `toloo-core` and is
//! re-exported here unchanged. This crate adds the I/O-bearing pieces:
//! pool storage, transports, request handling, trust, and key storage.

pub use toloo_core::{
    avatar, base64url, canonical, crypto, discovery, envelope, events, hex, ids, memzero, pow,
    private, rules, types, vectors,
};

#[cfg(test)]
pub(crate) use toloo_core::fixtures;

pub mod pool;
pub mod requests;
pub mod transport;
pub mod trust;

// Optional features
#[cfg(feature = "argon2")]
pub mod keystore;
