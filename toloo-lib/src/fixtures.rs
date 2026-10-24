//! Shared test fixtures. Compiled only for tests within this crate.

use std::collections::HashMap;

use serde_json::json;

use crate::base64url;
use crate::crypto::{ed25519_generate, x25519_generate};
use crate::envelope::sign_envelope;
use crate::types::{DatumBody, Envelope, Keypair, LocalNode, LocalRoom};

/// A fresh node with random signing + encryption keypairs.
pub fn mk_node() -> LocalNode {
    let (sig_pub, sig_seed) = ed25519_generate();
    let (enc_pub, enc_priv) = x25519_generate();
    LocalNode {
        sig: Keypair {
            pub_key: base64url::encode(&sig_pub),
            priv_key: base64url::encode(&sig_seed),
        },
        enc: Keypair {
            pub_key: base64url::encode(&enc_pub),
            priv_key: base64url::encode(&enc_priv),
        },
    }
}

/// A fresh room keypair.
pub fn mk_room() -> LocalRoom {
    let (room_pub, room_seed) = ed25519_generate();
    LocalRoom {
        sig: Keypair {
            pub_key: base64url::encode(&room_pub),
            priv_key: base64url::encode(&room_seed),
        },
    }
}

/// A signed, uncommitted room event.
pub fn mk_room_event(
    author: &LocalNode,
    room: &LocalRoom,
    event_type: &str,
    ts: u64,
    content: serde_json::Value,
) -> Envelope {
    let d = DatumBody {
        n: author.sig.pub_key.clone(),
        v: "0.2".to_owned(),
        t: event_type.to_owned(),
        ts,
        r: Some(room.sig.pub_key.clone()),
        to: None,
        c: Some(content),
        env: None,
        tc: None,
        exp: None,
        nonce: None,
        extra: HashMap::new(),
    };
    sign_envelope(d, &author.sig.priv_key).expect("sign should succeed")
}

/// A signed, uncommitted private message.
pub fn mk_private_message(sender: &LocalNode, recipient_sig: &str, ts: u64) -> Envelope {
    let d = DatumBody {
        n: sender.sig.pub_key.clone(),
        v: "0.2".to_owned(),
        t: "private.message".to_owned(),
        ts,
        r: None,
        to: Some(recipient_sig.to_owned()),
        c: Some(json!({"encrypted": "abc", "eph": sender.enc.pub_key})),
        env: None,
        tc: None,
        exp: None,
        nonce: None,
        extra: HashMap::new(),
    };
    sign_envelope(d, &sender.sig.priv_key).expect("sign should succeed")
}
