use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::memzero::Zeroize;

/// The only top-level protocol structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    pub d: DatumBody,
    pub p: String,
}

impl Default for DatumBody {
    /// A v0.2 datum skeleton; fill in `n`, `t`, `ts` and content fields.
    fn default() -> Self {
        Self {
            n: String::new(),
            v: "0.2".to_owned(),
            t: String::new(),
            ts: 0,
            r: None,
            to: None,
            c: None,
            env: None,
            tc: None,
            exp: None,
            nonce: None,
            extra: HashMap::new(),
            cached_canonical: OnceLock::new(),
            verified_layer: OnceLock::new(),
        }
    }
}

/// Signed datum inside an envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatumBody {
    pub n: String,
    pub v: String,
    pub t: String,
    pub ts: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Box<Envelope>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tc: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<u64>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,

    /// Cached canonical JSON bytes. Populated on first use.
    /// Not serialized — cleared on any mutation (use new DatumBody for mutations).
    #[serde(skip)]
    pub cached_canonical: OnceLock<Vec<u8>>,

    /// Cached verification result for commit/ack layer.
    /// When Some(true), the commit or ack signature (this layer) has been verified.
    /// Not serialized — set after successful verification.
    #[serde(skip)]
    pub verified_layer: OnceLock<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalNode {
    pub sig: Keypair,
    pub enc: Keypair,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalRoom {
    pub sig: Keypair,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Keypair {
    pub pub_key: String,
    pub priv_key: String,
}

// Manual Zeroize implementations for sensitive types.
impl Zeroize for Keypair {
    fn zeroize(&mut self) {
        self.priv_key.zeroize();
    }
}

impl Zeroize for LocalNode {
    fn zeroize(&mut self) {
        self.sig.zeroize();
        self.enc.zeroize();
    }
}

impl Zeroize for LocalRoom {
    fn zeroize(&mut self) {
        self.sig.zeroize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EndpointDescriptor {
    pub proto: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct: Option<bool>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}
