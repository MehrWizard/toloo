// Toloo Protocol v0.2 — core protocol + stateful runtime in one crate.
// Pure protocol modules have no IO; pool/transport/requests own state and network.

pub mod base64url;
pub mod canonical;
pub mod crypto;
pub mod envelope;
pub mod events;
pub mod hex;
pub mod ids;
pub mod pow;
pub mod private;
pub mod rules;
pub mod types;
pub mod vectors;
pub mod memzero;

#[cfg(test)]
pub(crate) mod fixtures;

pub mod discovery;
pub mod pool;
pub mod requests;
pub mod transport;
pub mod trust;

// Optional features
#[cfg(feature = "argon2")]
pub mod keystore;
