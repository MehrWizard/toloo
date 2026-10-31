//! Cross-protocol benchmarks: Toloo vs Signal (libsignal), Matrix (ruma +
//! vodozemac Olm), and Nostr (secp256k1 Schnorr + NIP-44).
//!
//! All libraries receive the same ~2 KB message body as the relay-side
//! `canonical/encode/medium` benchmark. Setup work (key generation, X3DH,
//! Olm sessions, signing for decrypt inputs) is untimed via `iter_batched`.
//! libsignal stores are `?Send`, so everything runs on one current-thread
//! Tokio runtime.

use std::sync::OnceLock;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use ed25519_dalek::{Signer, SigningKey};
use libsignal_protocol_syft::SignalMessage;
use toloo_lib::crypto::x25519_generate;
use toloo_lib::{base64url, private};
use toloo_protocol_bench::adapters::{matrix, nostr, signal};
use toloo_protocol_bench::{body, payload_json};

fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// Envelope serialization / event encoding for the non-Toloo protocols.
fn canonical_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_canonical");

    let matrix_object = matrix::event_object();
    group.bench_function("matrix", |b| {
        b.iter(|| matrix::canonical_encode(black_box(&matrix_object)).expect("canonical"))
    });

    let nostr_pair = nostr::NostrPair::new();
    let nostr_event = nostr_pair.build_signed(&body()).expect("nostr sign");
    group.bench_function("nostr", |b| {
        b.iter(|| nostr::NostrPair::event_json(black_box(&nostr_event)).expect("nostr json"))
    });

    group.finish();
}

/// Per-message signature verification.
fn verify_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_verify");

    let matrix_signer = matrix::MatrixSigner::new().expect("matrix signer");
    let mut matrix_signed = matrix::event_object();
    matrix_signer.sign(&mut matrix_signed).expect("matrix sign");
    group.bench_function("matrix", |b| {
        b.iter(|| matrix_signer.verify(black_box(&matrix_signed)).expect("verify"))
    });

    let nostr_pair = nostr::NostrPair::new();
    let nostr_event = nostr_pair.build_signed(&body()).expect("nostr sign");
    group.bench_function("nostr", |b| {
        b.iter(|| nostr_pair.verify(black_box(&nostr_event)).expect("verify"))
    });

    group.finish();
}

/// Wire-format decoding.
fn parse_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_parse");

    let matrix_object = matrix::event_object();
    let matrix_json = matrix::canonical_encode(&matrix_object).expect("canonical");
    group.bench_function("matrix", |b| {
        b.iter(|| matrix::parse_canonical(black_box(&matrix_json)).expect("parse"))
    });

    let nostr_pair = nostr::NostrPair::new();
    let nostr_event = nostr_pair.build_signed(&body()).expect("nostr sign");
    let nostr_json = nostr::NostrPair::event_json(&nostr_event).expect("nostr json");
    group.bench_function("nostr", |b| {
        b.iter(|| nostr::NostrPair::parse_event(black_box(&nostr_json)).expect("parse"))
    });

    // libsignal serializes protobuf on construction, so parsing is the only
    // standalone wire-format operation. Measure the steady-state (non
    // pre-key) message that the relay would carry for an established session.
    let mut pair = rt()
        .block_on(signal::SignalPair::new())
        .expect("signal pair");
    rt().block_on(pair.complete_handshake()).expect("signal handshake");
    let steady = rt()
        .block_on(pair.encrypt(body().as_bytes()))
        .expect("signal encrypt");
    let signal_bytes = steady.serialize().to_vec();
    group.bench_function("signal", |b| {
        b.iter(|| SignalMessage::try_from(black_box(signal_bytes.as_slice())).expect("parse"))
    });

    group.finish();
}

/// Event construction + signing.
fn build_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_build");

    let matrix_signer = matrix::MatrixSigner::new().expect("matrix signer");
    group.bench_function("matrix", |b| {
        b.iter_batched(
            matrix::event_object,
            |mut object| {
                matrix_signer.sign(black_box(&mut object)).expect("sign");
                object
            },
            BatchSize::SmallInput,
        )
    });

    let nostr_pair = nostr::NostrPair::new();
    let payload = body();
    group.bench_function("nostr", |b| {
        b.iter(|| nostr_pair.build_signed(black_box(&payload)).expect("sign"))
    });

    group.finish();
}

/// Raw ed25519 verification for the crypto card (same primitive Matrix uses).
fn crypto_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_crypto");

    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let verifying = signing.verifying_key();
    let message = body();
    let signature = signing.sign(message.as_bytes());
    group.bench_function("matrix_verify", |b| {
        b.iter(|| {
            verifying
                .verify_strict(black_box(message.as_bytes()), black_box(&signature))
                .is_ok()
        })
    });

    group.finish();
}

/// 1:1 private messages: Toloo X25519+ChaCha20-Poly1305, Signal Double
/// Ratchet, Matrix Olm, Nostr NIP-44.
fn private_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_private");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    let content = payload_json();
    let payload = body();

    let (recipient_pub, recipient_priv) = x25519_generate();
    let recipient_pub = base64url::encode(&recipient_pub);
    let recipient_priv = base64url::encode(&recipient_priv);

    group.bench_function("toloo_encrypt", |b| {
        b.iter(|| {
            private::encrypt_private(black_box(&content), black_box(&recipient_pub))
                .expect("encrypt")
        })
    });

    group.bench_function("toloo_decrypt", |b| {
        b.iter_batched(
            || private::encrypt_private(&content, &recipient_pub).expect("encrypt"),
            |ciphertext| {
                private::decrypt_private(black_box(&ciphertext), black_box(&recipient_priv))
                    .expect("decrypt")
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("signal_setup", |b| {
        b.iter_batched(
            || rt().block_on(signal::SignalPrepared::new()).expect("prepare"),
            |mut prepared| rt().block_on(prepared.establish()).expect("x3dh"),
            BatchSize::SmallInput,
        )
    });

    let mut signal_pair = rt()
        .block_on(signal::SignalPair::new())
        .expect("signal pair");
    rt().block_on(signal_pair.complete_handshake())
        .expect("signal handshake");
    group.bench_function("signal_encrypt", |b| {
        b.iter(|| {
            rt().block_on(signal_pair.encrypt(black_box(payload.as_bytes())))
                .expect("encrypt")
        })
    });

    group.bench_function("signal_decrypt", |b| {
        b.iter_batched(
            || {
                let mut pair = rt()
                    .block_on(signal::SignalPair::new())
                    .expect("signal pair");
                rt().block_on(pair.complete_handshake()).expect("handshake");
                let ciphertext = rt()
                    .block_on(pair.encrypt(black_box(payload.as_bytes())))
                    .expect("encrypt");
                (pair, ciphertext)
            },
            |(mut pair, ciphertext)| {
                rt().block_on(pair.decrypt(black_box(&ciphertext)))
                    .expect("decrypt")
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("olm_setup", |b| {
        b.iter_batched(
            matrix::OlmSetup::new,
            |setup| setup.create_outbound().expect("olm session"),
            BatchSize::SmallInput,
        )
    });

    let mut olm_pair = matrix::OlmPair::new().expect("olm pair");
    olm_pair.complete_handshake().expect("olm handshake");
    group.bench_function("olm_encrypt", |b| {
        b.iter(|| olm_pair.encrypt(black_box(payload.as_bytes())).expect("encrypt"))
    });

    group.bench_function("olm_decrypt", |b| {
        b.iter_batched(
            || {
                let mut pair = matrix::OlmPair::new().expect("olm pair");
                pair.complete_handshake().expect("olm handshake");
                let ciphertext = pair.encrypt(black_box(payload.as_bytes())).expect("encrypt");
                (pair, ciphertext)
            },
            |(mut pair, ciphertext)| {
                pair.decrypt(black_box(&ciphertext)).expect("decrypt")
            },
            BatchSize::SmallInput,
        )
    });

    let nostr_pair = nostr::NostrPair::new();
    group.bench_function("nostr_encrypt", |b| {
        b.iter(|| nostr_pair.encrypt(black_box(payload.as_bytes())).expect("encrypt"))
    });

    let nostr_ciphertext = nostr_pair.encrypt(payload.as_bytes()).expect("encrypt");
    group.bench_function("nostr_decrypt", |b| {
        b.iter(|| nostr_pair.decrypt(black_box(&nostr_ciphertext)).expect("decrypt"))
    });

    group.finish();
}

criterion_group!(
    benches,
    canonical_benches,
    verify_benches,
    parse_benches,
    build_benches,
    crypto_benches,
    private_benches
);
criterion_main!(benches);
