//! Benchmark-only cross-protocol harness.
//!
//! This crate exists to measure Toloo against other protocol libraries. It is
//! not published and is not part of the relay; the heavy dependencies are
//! behind the non-default `protocols` feature.

/// The shared message payload: a ~2 KB JSON body, matching the existing
/// `canonical/encode/medium` benchmark input size.
pub const BODY_LEN: usize = 1800;

pub fn body() -> String {
    "x".repeat(BODY_LEN)
}

pub fn payload_json() -> serde_json::Value {
    serde_json::json!({ "ch": 1, "body": body() })
}

#[cfg(feature = "protocols")]
pub mod adapters;
