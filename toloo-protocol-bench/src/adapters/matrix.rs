//! Matrix adapters: server-side event signing / canonical JSON (ruma) and
//! 1:1 Olm sessions (vodozemac).

use anyhow::{Context, Result};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use ruma_common::canonical_json::CanonicalJsonObject;
use ruma_common::serde::Base64;
use ruma_signatures::{sign_json, verify_json, Ed25519KeyPair, PublicKeyMap, PublicKeySet};
use serde_json::json;
use vodozemac::olm::{Account, OlmMessage, Session, SessionConfig};
use vodozemac::Curve25519PublicKey;

const SERVER: &str = "example.org";
const KEY_ID: &str = "1";

pub fn event_object() -> CanonicalJsonObject {
    let event = json!({
        "type": "m.room.message",
        "room_id": "!bench:example.org",
        "sender": "@alice:example.org",
        "origin_server_ts": 1_000_000u64,
        "content": { "msgtype": "m.text", "body": super::super::body() }
    });
    serde_json::from_value(event).expect("Matrix event is canonical-JSON compatible")
}

/// Signing key material for Matrix events.
pub struct MatrixSigner {
    key_pair: Ed25519KeyPair,
    public_keys: PublicKeyMap,
}

impl MatrixSigner {
    pub fn new() -> Result<Self> {
        let signing = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
        let der = signing.to_pkcs8_der().context("pkcs8 encode")?;
        let key_pair = Ed25519KeyPair::from_der(der.as_bytes(), KEY_ID.to_owned())
            .map_err(|e| anyhow::anyhow!("matrix key parse: {e}"))?;

        let mut key_set = PublicKeySet::new();
        key_set.insert(
            format!("ed25519:{KEY_ID}"),
            Base64::new(key_pair.public_key().to_vec()),
        );
        let mut public_keys = PublicKeyMap::new();
        public_keys.insert(SERVER.to_owned(), key_set);

        Ok(Self {
            key_pair,
            public_keys,
        })
    }

    pub fn sign(&self, object: &mut CanonicalJsonObject) -> Result<()> {
        sign_json(SERVER, &self.key_pair, object).map_err(|e| anyhow::anyhow!("sign_json: {e}"))?;
        Ok(())
    }

    pub fn verify(&self, object: &CanonicalJsonObject) -> Result<()> {
        verify_json(&self.public_keys, object)
            .map_err(|e| anyhow::anyhow!("verify_json: {e}"))?;
        Ok(())
    }
}

/// Canonical JSON serialization (Matrix signing form).
pub fn canonical_encode(object: &CanonicalJsonObject) -> Result<String> {
    ruma_signatures::to_canonical_json_string_for_signing(object)
        .map_err(|e| anyhow::anyhow!("canonical json: {e}"))
}

pub fn parse_canonical(json: &str) -> Result<CanonicalJsonObject> {
    serde_json::from_str(json).context("parse canonical json")
}

/// Accounts prepared for a 1:1 Olm session, with the outbound session not yet
/// created (so session setup can be timed on its own).
pub struct OlmSetup {
    pub alice: Account,
    pub bob: Account,
    pub one_time_key: Curve25519PublicKey,
}

impl Default for OlmSetup {
    fn default() -> Self {
        Self::new()
    }
}

impl OlmSetup {
    pub fn new() -> Self {
        let alice = Account::new();
        let mut bob = Account::new();
        bob.generate_one_time_keys(1);
        let one_time_key = *bob
            .one_time_keys()
            .values()
            .next()
            .expect("one-time key generated");
        bob.mark_keys_as_published();
        Self {
            alice,
            bob,
            one_time_key,
        }
    }

    pub fn create_outbound(&self) -> Result<Session> {
        self.alice
            .create_outbound_session(
                SessionConfig::version_1(),
                self.bob.curve25519_key(),
                self.one_time_key,
            )
            .map_err(|e| anyhow::anyhow!("olm session: {e}"))
    }
}

/// A 1:1 Olm session pair (vodozemac).
pub struct OlmPair {
    pub alice: Account,
    pub bob: Account,
    pub session: Session,
    pub bob_session: Option<Session>,
}

impl OlmPair {
    pub fn new() -> Result<Self> {
        let setup = OlmSetup::new();
        let session = setup.create_outbound()?;
        Ok(Self {
            alice: setup.alice,
            bob: setup.bob,
            session,
            bob_session: None,
        })
    }

    /// Exchange one untimed round trip so later decrypts are steady-state.
    pub fn complete_handshake(&mut self) -> Result<()> {
        let first = self.encrypt(b"handshake")?;
        self.decrypt(&first)?;
        Ok(())
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<OlmMessage> {
        self.session
            .encrypt(plaintext)
            .map_err(|e| anyhow::anyhow!("olm encrypt: {e}"))
    }

    pub fn decrypt(&mut self, message: &OlmMessage) -> Result<Vec<u8>> {
        if self.bob_session.is_none() {
            let OlmMessage::PreKey(pre_key_message) = message else {
                anyhow::bail!("first olm message must be a pre-key message");
            };
            let inbound = self.bob.create_inbound_session(
                SessionConfig::version_1(),
                self.alice.curve25519_key(),
                pre_key_message,
            )?;
            self.bob_session = Some(inbound.session);
            return Ok(inbound.plaintext);
        }
        let session = self.bob_session.as_mut().expect("session created");
        session
            .decrypt(message)
            .map_err(|e| anyhow::anyhow!("olm decrypt: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_and_canonical_round_trip() {
        let signer = MatrixSigner::new().expect("signer");
        let object = event_object();
        let mut signed = object.clone();
        signer.sign(&mut signed).expect("sign");
        signer.verify(&signed).expect("verify");
        let canonical = canonical_encode(&object).expect("canonical");
        let parsed = parse_canonical(&canonical).expect("parse");
        assert_eq!(parsed, object);
    }

    #[test]
    fn olm_round_trip() {
        let mut pair = OlmPair::new().expect("olm pair");
        let message = pair.encrypt(b"hello matrix").expect("encrypt");
        let plaintext = pair.decrypt(&message).expect("decrypt");
        assert_eq!(plaintext, b"hello matrix");
        let message = pair.encrypt(b"second").expect("encrypt");
        let plaintext = pair.decrypt(&message).expect("decrypt");
        assert_eq!(plaintext, b"second");
    }
}
