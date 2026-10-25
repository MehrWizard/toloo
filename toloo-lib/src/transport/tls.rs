//! Self-signed TLS certificate generation for relay server endpoints (wss, https).
//!
//! Certificates can be generated in two modes:
//! - Fresh (ephemeral): `make_self_signed_acceptor` generates a new keypair each call.
//! - Deterministic: `make_self_signed_acceptor_from_key` derives the cert from an existing
//!   Ed25519 signing key, producing a stable fingerprint across restarts.
//!
//! The SHA-256 fingerprint is included in the endpoint descriptor (`extra.cert_fp`)
//! so connecting peers can pin it instead of relying on a CA chain.

use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Result};
use der::Encode;
use ed25519_dalek::{pkcs8::EncodePrivateKey, SigningKey};
use getrandom;
use rustls::crypto::aws_lc_rs::Ticketer;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::ProducesTickets;
use rustls::ServerConfig;
use sha2::{Digest, Sha256};
use tokio_rustls::TlsAcceptor;
use x509_cert::{
    builder::{profile::BuilderProfile, Builder, CertificateBuilder, Result as BuilderResult},
    certificate::TbsCertificate,
    ext::{
        pkix::{
            AuthorityKeyIdentifier, BasicConstraints, ExtendedKeyUsage, KeyUsage, KeyUsages,
            SubjectAltName, SubjectKeyIdentifier,
        },
        Extension, ToExtension,
    },
    name::Name,
    serial_number::SerialNumber,
    spki::{SubjectPublicKeyInfo, SubjectPublicKeyInfoRef},
    time::{Time, Validity},
};

/// In-memory TLS session ticket encryption key (rotates on restart).
/// For production persistence across restarts, serialize the Ticketer state.
fn make_ticketer() -> Arc<dyn ProducesTickets> {
    Ticketer::new().map_err(|e| format!("Ticketer::new: {e}")).expect("Ticketer::new")
}

/// A simple self-signed certificate profile for TLS endpoints.
struct SelfSignedProfile {
    subject: Name,
    sans: Vec<SanType>,
}

impl SelfSignedProfile {
    fn new(hostname: &str) -> Result<Self> {
        // Subject CN = hostname (for display purposes; SAN is what matters for validation)
        let subject = Name::from_str(&format!("CN={}", hostname))
            .map_err(|e| anyhow!("invalid subject name: {}", e))?;

        // Build SANs: always include hostname, localhost, 127.0.0.1
        let mut sans = vec![SanType::DnsName(hostname.to_owned())];
        if hostname != "localhost" {
            sans.push(SanType::DnsName("localhost".to_owned()));
        }
        if hostname != "127.0.0.1" {
            sans.push(SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))));
        }

        Ok(Self { subject, sans })
    }
}

impl BuilderProfile for SelfSignedProfile {
    fn get_issuer(&self, subject: &Name) -> Name {
        // Self-signed: issuer == subject
        subject.clone()
    }

    fn get_subject(&self) -> Name {
        self.subject.clone()
    }

    fn build_extensions(
        &self,
        spk: SubjectPublicKeyInfoRef<'_>,
        _issuer_spk: SubjectPublicKeyInfoRef<'_>,
        tbs: &TbsCertificate,
    ) -> BuilderResult<Vec<Extension>> {
        let mut extensions = Vec::new();

        // Subject Key Identifier (RECOMMENDED for self-signed)
        let ski = SubjectKeyIdentifier::try_from(spk)?;
        extensions.push(ski.to_extension(&tbs.subject(), &extensions)?);

        // Authority Key Identifier (for self-signed, same as SKI)
        extensions.push(
            AuthorityKeyIdentifier {
                key_identifier: Some(ski.0.clone()),
                ..Default::default()
            }
            .to_extension(&tbs.subject(), &extensions)?,
        );

        // Basic Constraints: CA=false for TLS server cert
        extensions.push(
            BasicConstraints {
                ca: false,
                path_len_constraint: None,
            }
            .to_extension(&tbs.subject(), &extensions)?,
        );

        // Key Usage: digitalSignature, keyEncipherment (for TLS)
        let key_usage = KeyUsages::DigitalSignature | KeyUsages::KeyEncipherment;
        extensions.push(KeyUsage(key_usage).to_extension(&tbs.subject(), &extensions)?);

        // Extended Key Usage: serverAuth
        use const_oid::db::rfc5280::ID_KP_SERVER_AUTH;
        extensions.push(
            ExtendedKeyUsage(vec![ID_KP_SERVER_AUTH])
                .to_extension(&tbs.subject(), &extensions)?,
        );

        // Subject Alternative Names (CRITICAL for TLS)
        use x509_cert::ext::pkix::name::{GeneralName, GeneralNames};
        let mut gn: GeneralNames = Vec::new();
        for san in &self.sans {
            let gen_name = match san {
                SanType::DnsName(name) => {
                    GeneralName::DnsName(der::asn1::Ia5String::new(name)?)
                }
                SanType::IpAddress(ip) => GeneralName::from(*ip),
            };
            gn.push(gen_name);
        }
        let san_ext = SubjectAltName(gn);
        extensions.push(san_ext.to_extension(&tbs.subject(), &extensions)?);

        Ok(extensions)
    }
}

/// SAN type for our simple profile
enum SanType {
    DnsName(String),
    IpAddress(std::net::IpAddr),
}

/// Build a self-signed certificate from an existing Ed25519 seed (32 bytes).
/// Returns the Certificate DER and the PKCS#8 private key DER.
fn build_cert_from_ed25519_seed(seed: &[u8; 32], hostname: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let signing_key = SigningKey::from_bytes(seed);
    let verifying_key = signing_key.verifying_key();

    // Convert Ed25519 verifying key to SPKI (SubjectPublicKeyInfo)
    let spki = SubjectPublicKeyInfo::from_key(&verifying_key)
        .map_err(|e| anyhow!("failed to encode SPKI: {}", e))?;

    // PKCS#8 private key DER
    let pkcs8_der = signing_key
        .to_pkcs8_der()
        .map_err(|e| anyhow!("pkcs8 encoding failed: {}", e))?
        .to_bytes()
        .to_vec();

    // Build self-signed profile
    let profile = SelfSignedProfile::new(hostname)?;

    // Validity: 10 years (plenty for self-signed relay certs)
    let now = SystemTime::now();
    let not_before = Time::try_from(now).map_err(|e| anyhow!("time conversion: {}", e))?;
    let not_after = Time::try_from(now + Duration::from_secs(365 * 24 * 60 * 60 * 10))
        .map_err(|e| anyhow!("time conversion: {}", e))?;

    let validity = Validity::new(not_before, not_after);

    // Serial number: random 16 bytes
    let mut serial_bytes = [0u8; 16];
    getrandom::fill(&mut serial_bytes).map_err(|e| anyhow!("getrandom: {}", e))?;
    let serial_number = SerialNumber::new(&serial_bytes).map_err(|e| anyhow!("invalid serial: {}", e))?;

    // Build certificate
    let builder = CertificateBuilder::new(
        profile,
        serial_number,
        validity,
        spki,
    ).map_err(|e| anyhow!("cert builder: {}", e))?;

    // Sign with Ed25519
    let cert = builder.build(&signing_key).map_err(|e| anyhow!("cert sign: {}", e))?;

    // Encode to DER
    let cert_der = cert.to_der().map_err(|e| anyhow!("cert DER encode: {}", e))?;

    Ok((cert_der, pkcs8_der))
}

/// Generate a fresh self-signed TLS certificate for `hostname` and return:
/// - a ready-to-use `TlsAcceptor`
/// - the certificate DER bytes (for the invite URI's `extra.cert_der` field)
/// - the SHA-256 fingerprint as a lowercase hex string (for pinning)
///
/// The certificate includes SANs for `hostname`, plus `localhost` and
/// `127.0.0.1` so local testing always works without separate flags.
pub fn make_self_signed_acceptor(hostname: &str) -> Result<(TlsAcceptor, Vec<u8>, String), String> {
    // Generate a fresh Ed25519 seed for this certificate.
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("getrandom: {}", e))?;

    let (cert_der, key_der) = build_cert_from_ed25519_seed(&seed, hostname)
        .map_err(|e| format!("build cert: {}", e))?;

    // SHA-256 fingerprint (hex) for client-side pinning.
    let fingerprint = format!("{:x}", Sha256::digest(&cert_der));

    // Build rustls ServerConfig.
    let server_config = build_server_config(cert_der.clone(), key_der)
        .map_err(|e| format!("server config: {}", e))?;

    Ok((
        TlsAcceptor::from(Arc::new(server_config)),
        cert_der,
        fingerprint,
    ))
}

/// Generate a deterministic self-signed TLS certificate from an existing Ed25519 seed.
/// Returns:
/// - a ready-to-use `TlsAcceptor`
/// - the certificate DER bytes
/// - the SHA-256 fingerprint as a lowercase hex string (stable for the given seed)
///
/// Use this when the relay should have a stable identity across restarts
/// (e.g., the app relay using the user's loaded node identity).
pub fn make_self_signed_acceptor_from_key(
    ed25519_seed: &[u8; 32],
    hostname: &str,
) -> Result<(TlsAcceptor, Vec<u8>, String), String> {
    let (cert_der, key_der) = build_cert_from_ed25519_seed(ed25519_seed, hostname)
        .map_err(|e| format!("build cert: {}", e))?;

    // SHA-256 fingerprint (hex) for client-side pinning.
    let fingerprint = format!("{:x}", Sha256::digest(&cert_der));

    // Build rustls ServerConfig.
    let server_config = build_server_config(cert_der.clone(), key_der)
        .map_err(|e| format!("server config: {}", e))?;

    Ok((
        TlsAcceptor::from(Arc::new(server_config)),
        cert_der,
        fingerprint,
    ))
}

fn build_server_config(cert_der: Vec<u8>, key_der: Vec<u8>) -> Result<ServerConfig, String> {
    let cert_chain = vec![CertificateDer::from(cert_der)];
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));

    let mut config = ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("rustls versions: {}", e))?
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(|e| format!("rustls cert: {}", e))?;

    config.ticketer = make_ticketer();
    Ok(config)
}