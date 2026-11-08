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
fn room_lifecycle_and_requests_in_wasm() {
    let identity = contract::generate_identity().unwrap();
    let room = contract::generate_room_identity().unwrap();
    let room_id: Value = serde_json::from_str(&room).unwrap();
    let room_pub = room_id["sigPub"].as_str().unwrap();

    let pair: Value = serde_json::from_str(
        &contract::make_room_create(&identity, &room, Some("wasm room"), None).unwrap(),
    )
    .unwrap();
    let committed = pair["committed"].to_string();
    assert_eq!(contract::verify_chain_result(&committed), r#"{"ok":true}"#);

    let message = contract::make_room_message(&identity, room_pub, 1, "hello", None).unwrap();
    assert_eq!(contract::verify_chain_result(&message), r#"{"ok":true}"#);
    let info: Value = serde_json::from_str(&contract::envelope_info(&message).unwrap()).unwrap();
    assert_eq!(info["type"], "room.message");

    let sync = contract::request_events_sync(&identity, "[\"room\"]", Some(0), Some(10)).unwrap();
    let info: Value = serde_json::from_str(&contract::envelope_info(&sync).unwrap()).unwrap();
    assert_eq!(info["type"], "events.sync");

    let envelopes = json!([serde_json::from_str::<Value>(&message).unwrap()]).to_string();
    let uri = contract::encode_uri(&envelopes).unwrap();
    let decoded: Value = serde_json::from_str(&contract::decode_discovery(&uri).unwrap()).unwrap();
    assert_eq!(decoded.as_array().unwrap().len(), 1);
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
