//! Thin `wasm-bindgen` layer over the JSON contract.

use wasm_bindgen::prelude::*;

use crate::contract::{self, CachedPrivate};

fn js_err(error: anyhow::Error) -> JsError {
    JsError::new(&format!("{error:#}"))
}

#[wasm_bindgen(js_name = coreVersion)]
pub fn core_version() -> String {
    contract::core_version().to_owned()
}

#[wasm_bindgen(js_name = generateIdentity)]
pub fn generate_identity() -> Result<String, JsError> {
    contract::generate_identity().map_err(js_err)
}

#[wasm_bindgen(js_name = importIdentity)]
pub fn import_identity(sig_seed: &str, enc_priv: &str) -> Result<String, JsError> {
    contract::import_identity(sig_seed, enc_priv).map_err(js_err)
}

#[wasm_bindgen(js_name = makeNodeEnvelope)]
pub fn make_node_envelope(
    kind: &str,
    datum_json: &str,
    sig_pub: &str,
    sig_seed: &str,
    extra_json: Option<String>,
) -> Result<String, JsError> {
    contract::make_node_envelope(kind, datum_json, sig_pub, sig_seed, extra_json.as_deref())
        .map_err(js_err)
}

#[wasm_bindgen(js_name = verifyChain)]
pub fn verify_chain(envelope_json: &str) -> String {
    contract::verify_chain_result(envelope_json)
}

#[wasm_bindgen(js_name = verifyChains)]
pub fn verify_chains(envelopes_json: &str) -> Result<String, JsError> {
    contract::verify_chains_result(envelopes_json).map_err(js_err)
}

#[wasm_bindgen(js_name = envelopeInfo)]
pub fn envelope_info(envelope_json: &str) -> Result<String, JsError> {
    contract::envelope_info(envelope_json).map_err(js_err)
}

#[wasm_bindgen(js_name = canonicalize)]
pub fn canonicalize(json_str: &str) -> Result<String, JsError> {
    contract::canonicalize_json(json_str).map_err(js_err)
}

#[wasm_bindgen(js_name = privateEncrypt)]
pub fn private_encrypt(
    content_json: &str,
    sender_enc_priv: &str,
    sender_enc_pub: &str,
    recipient_enc_pub: &str,
) -> Result<String, JsError> {
    contract::private_encrypt(
        content_json,
        sender_enc_priv,
        sender_enc_pub,
        recipient_enc_pub,
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = privateDecrypt)]
pub fn private_decrypt(
    cipher_json: &str,
    recipient_enc_priv: &str,
    recipient_enc_pub: &str,
) -> Result<String, JsError> {
    contract::private_decrypt(cipher_json, recipient_enc_priv, recipient_enc_pub).map_err(js_err)
}

/// Pairwise-key cache for repeated private messages with the same peers.
#[wasm_bindgen(js_name = PairKeyCache)]
pub struct PairKeyCache {
    inner: CachedPrivate,
}

#[wasm_bindgen(js_class = PairKeyCache)]
impl PairKeyCache {
    #[wasm_bindgen(constructor)]
    pub fn new(capacity: usize) -> PairKeyCache {
        PairKeyCache {
            inner: CachedPrivate::new(capacity),
        }
    }

    pub fn encrypt(
        &mut self,
        content_json: &str,
        own_enc_priv: &str,
        own_enc_pub: &str,
        peer_enc_pub: &str,
    ) -> Result<String, JsError> {
        self.inner
            .encrypt(content_json, own_enc_priv, own_enc_pub, peer_enc_pub)
            .map_err(js_err)
    }

    pub fn decrypt(
        &mut self,
        cipher_json: &str,
        own_enc_priv: &str,
        own_enc_pub: &str,
    ) -> Result<String, JsError> {
        self.inner
            .decrypt(cipher_json, own_enc_priv, own_enc_pub)
            .map_err(js_err)
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}
