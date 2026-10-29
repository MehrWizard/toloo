use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use toloo_lib::base64url;
use toloo_lib::canonical::canonical;
use toloo_lib::crypto::{chacha20_decrypt, chacha20_encrypt, derive_private_message_key, ed25519_generate, ed25519_sign, ed25519_verify, ed25519_verify_batch, x25519_generate, x25519_shared_secret, get_verifying_key};
use toloo_lib::envelope::{make_envelope, parse_envelope, parse_envelope_bytes, verify_chain, envelope_to_json_bytes};
use toloo_lib::events::{make_node_meta, make_room_create, make_room_message};
use toloo_lib::types::{Keypair, LocalNode, LocalRoom};
use serde_json::json;

fn mk_node() -> LocalNode {
    let (sig_pub, sig_seed) = toloo_lib::crypto::ed25519_generate();
    let (enc_pub, enc_priv) = toloo_lib::crypto::x25519_generate();
    LocalNode {
        sig: Keypair { pub_key: toloo_lib::base64url::encode(&sig_pub), priv_key: toloo_lib::base64url::encode(&sig_seed) },
        enc: Keypair { pub_key: toloo_lib::base64url::encode(&enc_pub), priv_key: toloo_lib::base64url::encode(&enc_priv) },
    }
}

fn mk_room() -> LocalRoom {
    let (sig_pub, sig_seed) = toloo_lib::crypto::ed25519_generate();
    LocalRoom { sig: Keypair { pub_key: toloo_lib::base64url::encode(&sig_pub), priv_key: toloo_lib::base64url::encode(&sig_seed) } }
}

fn canonical_bench(c: &mut Criterion) {
    let node = mk_node();
    let _room = mk_room();

    // Small envelope (~200 bytes)
    let small = make_envelope(
        "room.message",
        Some(json!({"ch": 1, "body": "hello"})),
        &node,
        None,
    ).unwrap();

    // Medium envelope (~2KB)
    let medium = make_envelope(
        "room.message",
        Some(json!({"ch": 1, "body": "x".repeat(1800)})),
        &node,
        None,
    ).unwrap();

    // Large envelope (~10KB)
    let large = make_envelope(
        "room.message",
        Some(json!({"ch": 1, "body": "x".repeat(9500)})),
        &node,
        None,
    ).unwrap();

    let mut group = c.benchmark_group("canonical");
    group.throughput(Throughput::Bytes(1));

    for (name, env) in [("small", small), ("medium", medium), ("large", large)] {
        let size = serde_json::to_vec(&env).unwrap().len();
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("encode", name), &env, |b, env| {
            b.iter(|| canonical(black_box(env)))
        });
    }
    group.finish();
}

fn envelope_verify_bench(c: &mut Criterion) {
    let node = mk_node();
    let room = mk_room();

    // Create various envelope types
    let depth1 = make_envelope(
        "room.message",
        Some(json!({"ch": 1, "body": "hello world"})),
        &node,
        None,
    ).unwrap();

    let depth2 = {
        let _inner = make_room_message(&node, &room.sig.pub_key, 1, "hello world", vec![]).unwrap();
        // Wrap in commit (depth 2)
        let (_d1, commit) = make_room_create(&node, &room, Some("Test"), Some(json!([]))).unwrap();
        commit
    };

    let mut group = c.benchmark_group("envelope_verify");
    for (name, env) in [("depth1", depth1), ("depth2", depth2)] {
        group.bench_with_input(BenchmarkId::new("verify_chain", name), &env, |b, env| {
            b.iter(|| verify_chain(black_box(env)))
        });
    }
    group.finish();
}

fn envelope_parse_bench(c: &mut Criterion) {
    let node = mk_node();
    let env = make_envelope(
        "room.message",
        Some(json!({"ch": 1, "body": "hello world"})),
        &node,
        None,
    ).unwrap();
    let json_val = serde_json::to_value(&env).unwrap();
    let json_bytes = serde_json::to_vec(&env).unwrap();

    let mut group = c.benchmark_group("parse_envelope");

    // Old serde_json-based parsing (from Value)
    group.bench_function("parse_envelope_from_value", |b| {
        b.iter(|| parse_envelope(black_box(json_val.clone())))
    });

    // New zero-copy parsing from bytes (simd-json)
    group.bench_function("parse_envelope_bytes", |b| {
        b.iter(|| {
            let mut bytes = json_bytes.clone();
            parse_envelope_bytes(black_box(&mut bytes))
        })
    });

    group.finish();
}

fn envelope_serialize_bench(c: &mut Criterion) {
    let node = mk_node();
    let env = make_envelope(
        "room.message",
        Some(json!({"ch": 1, "body": "hello world"})),
        &node,
        None,
    ).unwrap();

    let mut group = c.benchmark_group("envelope_serialize");

    // Old serde_json-based serialization (to Value then to string)
    group.bench_function("to_envelope_json_old", |b| {
        b.iter(|| {
            let bytes = serde_json::to_vec(black_box(&env)).unwrap();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        })
    });

    // New manual serialization
    group.bench_function("envelope_to_json_bytes", |b| {
        b.iter(|| envelope_to_json_bytes(black_box(&env)))
    });

    group.finish();
}

fn event_build_bench(c: &mut Criterion) {
    let node = mk_node();
    let room = mk_room();

    let mut group = c.benchmark_group("event_build");

    group.bench_function("make_room_create", |b| {
        b.iter(|| make_room_create(black_box(&node), black_box(&room), Some("Test Room"), Some(json!([]))))
    });

    group.bench_function("make_room_message", |b| {
        b.iter(|| make_room_message(black_box(&node), black_box(&room.sig.pub_key), 1, "hello world", vec![]))
    });

    group.bench_function("make_node_meta", |b| {
        b.iter(|| make_node_meta(black_box(&node), vec![]))
    });

    group.finish();
}

fn pool_query_bench(c: &mut Criterion) {
    let pool = toloo_lib::pool::Pool::memory().unwrap();
    let node = mk_node();
    let room = mk_room();
    let room_pub = room.sig.pub_key.clone();

    // Pre-populate with 1000 events
    for i in 0..1000 {
        let env = make_room_message(&node, &room_pub, 1, &format!("message {}", i), vec![]).unwrap();
        pool.put(&env).unwrap();
    }

    let mut group = c.benchmark_group("pool_query");

    group.bench_function("get_room_channel_1k", |b| {
        b.iter(|| pool.get_room_channel(black_box(&room_pub), black_box(1)))
    });

    group.bench_function("get_wanted_after_0", |b| {
        b.iter(|| pool.get_wanted(black_box(&[room_pub.as_str()]), black_box(&[]), black_box(0), black_box(100)))
    });

    group.bench_function("get_wanted_after_tc", |b| {
        let latest_tc = pool.latest_tc(&room_pub).unwrap();
        b.iter(|| pool.get_wanted(black_box(&[room_pub.as_str()]), black_box(&[]), black_box(latest_tc / 2), black_box(100)))
    });

    group.bench_function("get_by_id", |b| {
        let events = pool.get_room_channel(&room_pub, 1).unwrap();
        let ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        b.iter(|| pool.get_by_datum_ids(black_box(&ids.iter().map(|s| s.as_str()).collect::<Vec<_>>())))
    });

    group.finish();
}

fn pool_write_bench(c: &mut Criterion) {
    let node = mk_node();
    let room = mk_room();
    let room_pub = room.sig.pub_key.clone();

    // Benchmark single put
    let mut group = c.benchmark_group("pool_write");

    group.bench_function("put_single", |b| {
        let pool = toloo_lib::pool::Pool::memory().unwrap();
        b.iter(|| {
            let env = make_room_message(&node, &room_pub, 1, "benchmark message", vec![]).unwrap();
            pool.put(black_box(&env))
        })
    });

    // Benchmark batch put (100 events)
    group.bench_function("put_batch_100", |b| {
        b.iter(|| {
            let pool = toloo_lib::pool::Pool::memory().unwrap();
            let envelopes: Vec<_> = (0..100)
                .map(|i| make_room_message(&node, &room_pub, 1, &format!("msg {}", i), vec![]).unwrap())
                .collect();
            let refs: Vec<_> = envelopes.iter().collect();
            pool.put_batch(black_box(&refs))
        })
    });

    // Benchmark batch put (10 events)
    group.bench_function("put_batch_10", |b| {
        b.iter(|| {
            let pool = toloo_lib::pool::Pool::memory().unwrap();
            let envelopes: Vec<_> = (0..10)
                .map(|i| make_room_message(&node, &room_pub, 1, &format!("msg {}", i), vec![]).unwrap())
                .collect();
            let refs: Vec<_> = envelopes.iter().collect();
            pool.put_batch(black_box(&refs))
        })
    });

    group.finish();
}

fn crypto_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto");

    group.bench_function("ed25519_generate", |b| {
        b.iter(|| ed25519_generate())
    });

    let (pub_key, seed) = ed25519_generate();
    let msg = b"benchmark message";

    group.bench_function("ed25519_sign", |b| {
        b.iter(|| ed25519_sign(black_box(msg), black_box(&seed)))
    });

    let sig = ed25519_sign(msg, &seed).unwrap();
    let pub_key_b64 = base64url::encode(&pub_key);
    group.bench_function("ed25519_verify", |b| {
        b.iter(|| ed25519_verify(black_box(msg), black_box(&sig), black_box(&pub_key_b64)))
    });

    // Batch verification benchmark
    let vk = get_verifying_key(&pub_key_b64).unwrap();
    let msg_refs: Vec<&[u8]> = (0..10).map(|_| msg.as_slice()).collect();
    let sig_refs: Vec<&[u8]> = (0..10).map(|_| sig.as_slice()).collect();
    let vk_refs: Vec<&ed25519_dalek::VerifyingKey> = (0..10).map(|_| &vk).collect();

    group.bench_function("ed25519_verify_batch_10", |b| {
        b.iter(|| {
            ed25519_verify_batch(
                msg_refs.iter().zip(sig_refs.iter()).zip(vk_refs.iter()).map(|((m, s), vk)| (*m, *s, *vk))
            )
        })
    });

    group.bench_function("x25519_generate", |b| {
        b.iter(|| x25519_generate())
    });

    let (_pub_a, priv_a) = x25519_generate();
    let (pub_b, _priv_b) = x25519_generate();
    group.bench_function("x25519_shared_secret", |b| {
        b.iter(|| x25519_shared_secret(black_box(&priv_a), black_box(&pub_b)))
    });

    let shared = x25519_shared_secret(&priv_a, &pub_b).unwrap();
    let (key, nonce) = derive_private_message_key(&shared, &[0u8; 32]);
    let plaintext = b"benchmark plaintext data";
    let ciphertext = chacha20_encrypt(&key, &nonce, plaintext);

    group.bench_function("chacha20_encrypt", |b| {
        b.iter(|| chacha20_encrypt(black_box(&key), black_box(&nonce), black_box(plaintext)))
    });

    group.bench_function("chacha20_decrypt", |b| {
        b.iter(|| chacha20_decrypt(black_box(&key), black_box(&nonce), black_box(&ciphertext)))
    });

    group.finish()
}

criterion_group!(
    benches,
    canonical_bench,
    envelope_verify_bench,
    envelope_parse_bench,
    envelope_serialize_bench,
    event_build_bench,
    pool_query_bench,
    pool_write_bench,
    crypto_bench
);
criterion_main!(benches);