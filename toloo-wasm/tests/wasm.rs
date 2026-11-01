#![cfg(target_arch = "wasm32")]

use serde_json::{json, Value};
use toloo_wasm::contract;
use wasm_bindgen_test::*;

fn identity() -> Value {
    serde_json::from_str(&contract::generate_identity().unwrap()).unwrap()
}

#[wasm_bindgen_test]
fn identity_round_trip_in_wasm() {
    let original = identity();
    let imported = contract::import_identity(
        original["sigSeed"].as_str().unwrap(),
        original["encPriv"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&imported).unwrap(), original);
}

#[wasm_bindgen_test]
fn sign_and_verify_in_wasm() {
    let id = identity();
    let envelope = contract::make_node_envelope(
        "node.meta",
        r#"{"name":"wasm"}"#,
        id["sigPub"].as_str().unwrap(),
        id["sigSeed"].as_str().unwrap(),
        Some(r#"{"tc":123}"#),
    )
    .unwrap();

    assert_eq!(contract::verify_chain_result(&envelope), r#"{"ok":true}"#);
    let info: Value = serde_json::from_str(&contract::envelope_info(&envelope).unwrap()).unwrap();
    assert_eq!(info["type"], "node.meta");
    assert!(info["ts"].as_u64().unwrap() > 0, "web-time clock works");
}

#[wasm_bindgen_test]
fn batch_verification_in_wasm() {
    let id = identity();
    let one = contract::make_node_envelope(
        "node.meta",
        r#"{"name":"one"}"#,
        id["sigPub"].as_str().unwrap(),
        id["sigSeed"].as_str().unwrap(),
        None,
    )
    .unwrap();
    let valid: Value = serde_json::from_str(&one).unwrap();
    let mut tampered = valid.clone();
    tampered["d"]["c"]["name"] = json!("two");

    let verdicts: Vec<Value> = serde_json::from_str(
        &contract::verify_chains_result(&json!([valid, tampered]).to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(verdicts[0]["ok"], true);
    assert_eq!(verdicts[1]["ok"], false);
}

#[wasm_bindgen_test]
fn cached_private_messages_in_wasm() {
    let alice = identity();
    let bob = identity();
    let mut alice_cache = contract::CachedPrivate::new(4);
    let mut bob_cache = contract::CachedPrivate::new(4);

    let cipher = alice_cache
        .encrypt(
            r#"{"body":"hello from wasm"}"#,
            alice["encPriv"].as_str().unwrap(),
            alice["encPub"].as_str().unwrap(),
            bob["encPub"].as_str().unwrap(),
        )
        .unwrap();
    let plaintext = bob_cache
        .decrypt(
            &cipher,
            bob["encPriv"].as_str().unwrap(),
            bob["encPub"].as_str().unwrap(),
        )
        .unwrap();
    let content: Value = serde_json::from_str(&plaintext).unwrap();
    assert_eq!(content["body"], "hello from wasm");
}
