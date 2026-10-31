//! libsignal (Signal Protocol) adapter: X3DH session setup + Double Ratchet.
//!
//! The setup mirrors libsignal's own test support: Bob publishes an identity,
//! a one-time prekey, a signed prekey, and a Kyber prekey; Alice runs X3DH via
//! `process_prekey_bundle`; both then exchange ratchet messages.

use std::time::SystemTime;

use anyhow::Result;
use libsignal_protocol_syft::{
    kem, message_decrypt, message_encrypt, process_prekey_bundle, CiphertextMessage, DeviceId,
    GenericSignedPreKey, IdentityKeyPair, IdentityKeyStore, InMemSignalProtocolStore, KeyPair,
    KyberPreKeyId, KyberPreKeyRecord, KyberPreKeyStore, PreKeyBundle, PreKeyId, PreKeyRecord,
    PreKeyStore, ProtocolAddress, SignedPreKeyId, SignedPreKeyRecord, SignedPreKeyStore, Timestamp,
};
use rand::rngs::OsRng;
use rand::TryRngCore as _;

const REGISTRATION_ID: u32 = 1;

/// A published prekey bundle plus both in-memory stores, before X3DH.
pub struct SignalPrepared {
    pub alice: InMemSignalProtocolStore,
    pub bob: InMemSignalProtocolStore,
    pub alice_address: ProtocolAddress,
    pub bob_address: ProtocolAddress,
    pub bundle: PreKeyBundle,
}

impl SignalPrepared {
    pub async fn new() -> Result<Self> {
        let mut rng = OsRng.unwrap_err();
        let alice_identity = IdentityKeyPair::generate(&mut rng);
        let bob_identity = IdentityKeyPair::generate(&mut rng);

        let alice = InMemSignalProtocolStore::new(alice_identity, REGISTRATION_ID)?;
        let mut bob = InMemSignalProtocolStore::new(bob_identity, REGISTRATION_ID + 1)?;

        let device = DeviceId::new(1).expect("valid device id");
        let alice_address = ProtocolAddress::new("alice".to_owned(), device);
        let bob_address = ProtocolAddress::new("bob".to_owned(), device);

        let bob_identity = bob.identity_store.get_identity_key_pair().await?;

        let pre_key_pair = KeyPair::generate(&mut rng);
        let signed_pre_key_pair = KeyPair::generate(&mut rng);
        let kyber_pre_key_pair = kem::KeyPair::generate(kem::KeyType::Kyber1024, &mut rng);

        let signed_pre_key_public = signed_pre_key_pair.public_key.serialize();
        let signed_pre_key_signature = bob_identity
            .private_key()
            .calculate_signature(&signed_pre_key_public, &mut rng)?;
        let kyber_pre_key_public = kyber_pre_key_pair.public_key.serialize();
        let kyber_pre_key_signature = bob_identity
            .private_key()
            .calculate_signature(&kyber_pre_key_public, &mut rng)?;

        let pre_key_id = PreKeyId::from(1u32);
        let signed_pre_key_id = SignedPreKeyId::from(2u32);
        let kyber_pre_key_id = KyberPreKeyId::from(3u32);

        let bundle = PreKeyBundle::new(
            bob.identity_store.get_local_registration_id().await?,
            device,
            Some((pre_key_id, pre_key_pair.public_key)),
            signed_pre_key_id,
            signed_pre_key_pair.public_key,
            signed_pre_key_signature.to_vec(),
            kyber_pre_key_id,
            kyber_pre_key_pair.public_key.clone(),
            kyber_pre_key_signature.to_vec(),
            *bob_identity.identity_key(),
        )?;

        let timestamp = Timestamp::from_epoch_millis(1);
        bob.save_pre_key(pre_key_id, &PreKeyRecord::new(pre_key_id, &pre_key_pair))
            .await?;
        bob.save_signed_pre_key(
            signed_pre_key_id,
            &SignedPreKeyRecord::new(
                signed_pre_key_id,
                timestamp,
                &signed_pre_key_pair,
                &signed_pre_key_signature,
            ),
        )
        .await?;
        bob.save_kyber_pre_key(
            kyber_pre_key_id,
            &KyberPreKeyRecord::new(
                kyber_pre_key_id,
                timestamp,
                &kyber_pre_key_pair,
                &kyber_pre_key_signature,
            ),
        )
        .await?;

        Ok(Self {
            alice,
            bob,
            alice_address,
            bob_address,
            bundle,
        })
    }

    /// Run the X3DH initiator half (timed separately in the setup benchmark).
    pub async fn establish(&mut self) -> Result<()> {
        let mut rng = OsRng.unwrap_err();
        process_prekey_bundle(
            &self.bob_address,
            &mut self.alice.session_store,
            &mut self.alice.identity_store,
            &self.bundle,
            SystemTime::now(),
            &mut rng,
        )
        .await?;
        Ok(())
    }

    pub async fn into_pair(self) -> Result<SignalPair> {
        let mut prepared = self;
        prepared.establish().await?;
        Ok(SignalPair {
            alice: prepared.alice,
            bob: prepared.bob,
            alice_address: prepared.alice_address,
            bob_address: prepared.bob_address,
        })
    }
}

/// An established Signal session pair.
pub struct SignalPair {
    pub alice: InMemSignalProtocolStore,
    pub bob: InMemSignalProtocolStore,
    pub alice_address: ProtocolAddress,
    pub bob_address: ProtocolAddress,
}

impl SignalPair {
    pub async fn new() -> Result<Self> {
        SignalPrepared::new().await?.into_pair().await
    }

    pub async fn encrypt(&mut self, plaintext: &[u8]) -> Result<CiphertextMessage> {
        let mut rng = OsRng.unwrap_err();
        let ct = message_encrypt(
            plaintext,
            &self.bob_address,
            &mut self.alice.session_store,
            &mut self.alice.identity_store,
            SystemTime::now(),
            &mut rng,
        )
        .await?;
        Ok(ct)
    }

    pub async fn decrypt(&mut self, ciphertext: &CiphertextMessage) -> Result<Vec<u8>> {
        let mut rng = OsRng.unwrap_err();
        let pt = message_decrypt(
            ciphertext,
            &self.alice_address,
            &mut self.bob.session_store,
            &mut self.bob.identity_store,
            &mut self.bob.pre_key_store,
            &self.bob.signed_pre_key_store,
            &mut self.bob.kyber_pre_key_store,
            &mut rng,
        )
        .await?;
        Ok(pt)
    }

    /// Bob -> Alice (the Double Ratchet is symmetric once established).
    pub async fn encrypt_reverse(&mut self, plaintext: &[u8]) -> Result<CiphertextMessage> {
        let mut rng = OsRng.unwrap_err();
        let ct = message_encrypt(
            plaintext,
            &self.alice_address,
            &mut self.bob.session_store,
            &mut self.bob.identity_store,
            SystemTime::now(),
            &mut rng,
        )
        .await?;
        Ok(ct)
    }

    pub async fn decrypt_reverse(&mut self, ciphertext: &CiphertextMessage) -> Result<Vec<u8>> {
        let mut rng = OsRng.unwrap_err();
        let pt = message_decrypt(
            ciphertext,
            &self.bob_address,
            &mut self.alice.session_store,
            &mut self.alice.identity_store,
            &mut self.alice.pre_key_store,
            &self.alice.signed_pre_key_store,
            &mut self.alice.kyber_pre_key_store,
            &mut rng,
        )
        .await?;
        Ok(pt)
    }

    /// Exchange one untimed round trip so later messages are steady-state
    /// `SignalMessage`s instead of pre-key messages.
    pub async fn complete_handshake(&mut self) -> Result<()> {
        let first = self.encrypt(b"handshake").await?;
        self.decrypt(&first).await?;
        let reply = self.encrypt_reverse(b"handshake-ack").await?;
        self.decrypt_reverse(&reply).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip() {
        let mut pair = SignalPair::new().await.expect("session setup");
        let ct = pair.encrypt(b"hello signal").await.expect("encrypt");
        let pt = pair.decrypt(&ct).await.expect("decrypt");
        assert_eq!(pt, b"hello signal");

        pair.complete_handshake().await.expect("handshake");
        let ct = pair.encrypt(b"steady state").await.expect("encrypt");
        assert!(matches!(ct, CiphertextMessage::SignalMessage(_)));
        let pt = pair.decrypt(&ct).await.expect("decrypt");
        assert_eq!(pt, b"steady state");
    }
}
