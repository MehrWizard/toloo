//! Nostr adapter: event build/sign/verify/parse (secp256k1 Schnorr) and
//! NIP-44 1:1 encryption.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use nostr::nips::nip44::v2::{
    decrypt_to_bytes as decrypt_v2, encrypt_to_bytes_with_nonce, ConversationKey,
};
use nostr::prelude::*;

pub struct NostrPair {
    sender: Keys,
    recipient: Keys,
}

impl Default for NostrPair {
    fn default() -> Self {
        Self::new()
    }
}

impl NostrPair {
    pub fn new() -> Self {
        Self {
            sender: Keys::generate(),
            recipient: Keys::generate(),
        }
    }

    pub fn build_signed(&self, content: &str) -> Result<Event> {
        FinalizeEvent::finalize(EventBuilder::new(Kind::TextNote, content), &self.sender)
            .context("nostr sign")
    }

    pub fn verify(&self, event: &Event) -> Result<()> {
        event.verify().context("nostr verify")
    }

    pub fn event_json(event: &Event) -> Result<String> {
        serde_json::to_string(event).context("nostr event json")
    }

    pub fn parse_event(json: &str) -> Result<Event> {
        serde_json::from_str(json).context("nostr event parse")
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        nip44::encrypt(
            self.sender.secret_key(),
            &self.recipient.public_key(),
            plaintext,
            Version::V2,
        )
        .context("nip44 encrypt")
    }

    pub fn decrypt(&self, ciphertext: &str) -> Result<Vec<u8>> {
        nip44::decrypt_to_bytes(
            self.recipient.secret_key(),
            &self.sender.public_key(),
            ciphertext,
        )
        .context("nip44 decrypt")
    }

    /// Derive the NIP-44 conversation key once for a peer (one ECDH).
    pub fn conversation_key(&self) -> Result<ConversationKey> {
        ConversationKey::derive(self.sender.secret_key(), &self.recipient.public_key())
            .context("nip44 conversation key")
    }

    /// Encrypt with a cached conversation key: no ECDH per message.
    pub fn encrypt_cached(&self, key: &ConversationKey, plaintext: &[u8]) -> Result<String> {
        let nonce = rand::random::<[u8; 32]>();
        let payload =
            encrypt_to_bytes_with_nonce(key, plaintext, nonce).context("nip44 encrypt cached")?;
        Ok(STANDARD.encode(payload))
    }

    /// Decrypt with a cached conversation key: no ECDH per message.
    pub fn decrypt_cached(&self, key: &ConversationKey, ciphertext: &str) -> Result<Vec<u8>> {
        let payload = STANDARD
            .decode(ciphertext)
            .context("nip44 cached base64 decode")?;
        decrypt_v2(key, &payload).context("nip44 decrypt cached")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_round_trip() {
        let pair = NostrPair::new();
        let event = pair.build_signed("hello nostr").expect("sign");
        pair.verify(&event).expect("verify");
        let json = NostrPair::event_json(&event).expect("json");
        let parsed = NostrPair::parse_event(&json).expect("parse");
        assert_eq!(parsed.id, event.id);
        pair.verify(&parsed).expect("verify parsed");
    }

    #[test]
    fn nip44_round_trip() {
        let pair = NostrPair::new();
        let ciphertext = pair.encrypt(b"hello nip44").expect("encrypt");
        let plaintext = pair.decrypt(&ciphertext).expect("decrypt");
        assert_eq!(plaintext, b"hello nip44");
    }
}
