use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use toloo_lib::pool::BlocklistEntry;
use toloo_lib::types::{LocalNode, LocalRoom};
use toloo_lib::transport::server::RelayMetrics;

// ══════════════════════════════════════════════════════════════════════
// App config store (single JSON file, persisted across restarts)
// ══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelayConfigRecord {
    pub id:      String,
    pub proto:   String,
    pub host:    String,
    pub port:    u16,
    pub path:    Option<String>,
    pub direct:  bool,
    /// Whether this relay was running when the app was last closed.
    pub active:  bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BlockEntry {
    node_pub:   String,
    kind:       String,
    reason:     Option<String>,
    blocked_at: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct AppConfig {
    #[serde(default)]
    relays:    Vec<RelayConfigRecord>,
    #[serde(default)]
    blocklist: Vec<BlockEntry>,
}

pub struct AppDb {
    path: std::path::PathBuf,
    state: Mutex<AppConfig>,
}

impl AppDb {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let state = if path.exists() {
            let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            serde_json::from_str(&raw).map_err(|e| e.to_string())?
        } else {
            AppConfig::default()
        };
        Ok(Self { path: path.to_owned(), state: Mutex::new(state) })
    }

    fn persist(&self, cfg: &AppConfig) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn save(&self, r: &RelayConfigRecord) -> Result<(), String> {
        let mut cfg = self.state.lock().unwrap();
        if let Some(slot) = cfg.relays.iter_mut().find(|x| x.id == r.id) {
            *slot = r.clone();
        } else {
            cfg.relays.push(r.clone());
        }
        self.persist(&cfg)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut cfg = self.state.lock().unwrap();
        cfg.relays.retain(|x| x.id != id);
        self.persist(&cfg)
    }

    pub fn list(&self) -> Result<Vec<RelayConfigRecord>, String> {
        Ok(self.state.lock().unwrap().relays.clone())
    }

    pub fn set_active(&self, id: &str, active: bool) -> Result<(), String> {
        let mut cfg = self.state.lock().unwrap();
        if let Some(r) = cfg.relays.iter_mut().find(|x| x.id == id) {
            r.active = active;
        }
        self.persist(&cfg)
    }

    pub fn block_node(&self, node_pub: &str, kind: &str, reason: Option<&str>, now: u64) -> Result<(), String> {
        let mut cfg = self.state.lock().unwrap();
        if let Some(slot) = cfg.blocklist.iter_mut().find(|b| b.node_pub == node_pub) {
            slot.kind = kind.to_owned();
            slot.reason = reason.map(|r| r.to_owned());
            slot.blocked_at = now;
        } else {
            cfg.blocklist.push(BlockEntry {
                node_pub: node_pub.to_owned(),
                kind: kind.to_owned(),
                reason: reason.map(|r| r.to_owned()),
                blocked_at: now,
            });
        }
        self.persist(&cfg)
    }

    pub fn unblock_node(&self, node_pub: &str) -> Result<bool, String> {
        let mut cfg = self.state.lock().unwrap();
        let before = cfg.blocklist.len();
        cfg.blocklist.retain(|b| b.node_pub != node_pub);
        let removed = cfg.blocklist.len() != before;
        if removed {
            self.persist(&cfg)?;
        }
        Ok(removed)
    }

    pub fn get_blocklist(&self) -> Result<Vec<BlocklistEntry>, String> {
        let cfg = self.state.lock().unwrap();
        let mut out: Vec<BlocklistEntry> = cfg.blocklist.iter()
            .map(|b| (b.node_pub.clone(), b.kind.clone(), b.reason.clone(), b.blocked_at))
            .collect();
        out.sort_by_key(|(_, _, _, at)| std::cmp::Reverse(*at));
        Ok(out)
    }

    pub fn is_blocked(&self, node_pub: &str) -> Result<bool, String> {
        let cfg = self.state.lock().unwrap();
        Ok(cfg.blocklist.iter().any(|b| b.node_pub == node_pub))
    }
}

// ══════════════════════════════════════════════════════════════════════
// App state
// ══════════════════════════════════════════════════════════════════════

pub struct LocalRelayEntry {
    pub runtime_id: u32,
    pub config_id:  String,
    pub stop_tx:    tokio::sync::oneshot::Sender<()>,
    pub invite_uri: String,
    pub proto:      String,
    pub host:       String,
    pub port:       u16,
    pub metrics:    Arc<RelayMetrics>,
}

pub struct AppState {
    pub node:          Mutex<Option<LocalNode>>,
    pub rooms:         Mutex<HashMap<String, LocalRoom>>,  // room_pub → LocalRoom (key held by this node)
    pub relays:        Mutex<Vec<LocalRelayEntry>>,
    pub relay_id_next: Mutex<u32>,
    pub db:            Mutex<Option<AppDb>>,
}

// ══════════════════════════════════════════════════════════════════════
// Commands
// ══════════════════════════════════════════════════════════════════════

mod commands {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::Arc;

    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use toloo_lib::base64url;
    use toloo_lib::crypto::{ed25519_generate, x25519_generate};
    use toloo_lib::envelope::{depth, innermost, parse_envelope, verify_chain, wrap_ack};
    use toloo_lib::events::{make_node_meta, make_room_create, make_room_join, make_room_message, make_side_attestation, make_room_flag};
    use toloo_lib::keystore;
    use toloo_lib::ids::eid;
    use toloo_lib::types::{EndpointDescriptor, Envelope, Keypair, LocalNode, LocalRoom};
    use toloo_lib::discovery::{
        decode as discovery_decode,
        encode_file as discovery_encode_file,
        encode_uri as discovery_encode_uri,
    };
    use toloo_lib::pool::Pool;
    use toloo_lib::transport::client::{pool_exchange as lib_pool_exchange, PoolExchangeConfig, TlsVerify, WantRoom};
    use toloo_lib::transport::server::{EndpointConfig, run_relay, RelayConfig, RelayMetrics};
    use toloo_lib::transport::tls::make_self_signed_acceptor_from_key;
    
    use super::{AppDb, AppState, LocalRelayEntry, RelayConfigRecord};

    // ─── Return types ─────────────────────────────────────────────────

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct RelayInfo {
        pub runtime_id: u32,
        pub config_id:  String,
        pub invite_uri: String,
        pub proto:      String,
        pub host:       String,
        pub port:       u16,
        pub active:     usize,
        pub total:      usize,
        pub bytes_in:   u64,
        pub bytes_out:  u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct NodeInfo {
        pub sig_pub:   String,
        pub enc_pub:   String,
        pub node_json: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct MessageItem {
        pub eid:           String,
        pub author:        String,
        pub body:          String,
        pub ts:            u64,
        pub depth:         u8,
        pub envelope_json: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct InviteInfo {
        pub room_pub:       String,
        pub room_name:      Option<String>,
        pub creator:        String,
        pub invite_uri:     String,
        pub envelope_jsons: Vec<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct BlocklistItem {
        pub node_pub:   String,
        pub kind:       String,
        pub reason:     Option<String>,
        pub blocked_at: u64,
    }

    // ─── Helpers ──────────────────────────────────────────────────────

    fn to_err<E: std::fmt::Display>(e: E) -> String { e.to_string() }

    fn node_info(node: &LocalNode) -> Result<NodeInfo, String> {
        Ok(NodeInfo {
            sig_pub:   node.sig.pub_key.clone(),
            enc_pub:   node.enc.pub_key.clone(),
            node_json: serde_json::to_string(node).map_err(to_err)?,
        })
    }

    fn parse_env_str(json: &str) -> Result<Envelope, String> {
        let v: Value = serde_json::from_str(json).map_err(to_err)?;
        parse_envelope(v).map_err(to_err)
    }

    fn serialize_env(env: &Envelope) -> Result<String, String> {
        serde_json::to_string(env).map_err(to_err)
    }

    fn message_item(env: &Envelope) -> Option<MessageItem> {
        let inner = innermost(env);
        if inner.d.t != "room.message" { return None; }
        let body = inner.d.c.as_ref()?.get("body")?.as_str()?.to_owned();
        Some(MessageItem {
            eid:           eid(env),
            author:        inner.d.n.clone(),
            body,
            ts:            inner.d.ts,
            depth:         depth(env),
            envelope_json: serde_json::to_string(env).ok()?,
        })
    }

    fn throwaway_node() -> LocalNode {
        let (sig_pub, sig_seed) = ed25519_generate();
        let (enc_pub, enc_priv) = x25519_generate();
        LocalNode {
            sig: Keypair { pub_key: base64url::encode(&sig_pub),  priv_key: base64url::encode(&sig_seed) },
            enc: Keypair { pub_key: base64url::encode(&enc_pub),  priv_key: base64url::encode(&enc_priv) },
        }
    }

    fn relay_info(entry: &LocalRelayEntry) -> RelayInfo {
        use std::sync::atomic::Ordering;
        RelayInfo {
            runtime_id: entry.runtime_id,
            config_id:  entry.config_id.clone(),
            invite_uri: entry.invite_uri.clone(),
            proto:      entry.proto.clone(),
            host:       entry.host.clone(),
            port:       entry.port,
            active:     entry.metrics.active.load(Ordering::Relaxed),
            total:      entry.metrics.total.load(Ordering::Relaxed),
            bytes_in:   entry.metrics.bytes_in.load(Ordering::Relaxed),
            bytes_out:  entry.metrics.bytes_out.load(Ordering::Relaxed),
        }
    }

    fn with_db<F, T>(state: &AppState, f: F) -> Result<T, String>
    where F: FnOnce(&AppDb) -> Result<T, String>
    {
        let guard = state.db.lock().unwrap();
        let db = guard.as_ref().ok_or_else(|| "relay DB not initialised".to_owned())?;
        f(db)
    }

    fn detect_lan_ip() -> String {
        use std::net::UdpSocket;
        if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
            if s.connect("8.8.8.8:80").is_ok() {
                if let Ok(addr) = s.local_addr() {
                    return addr.ip().to_string();
                }
            }
        }
        "127.0.0.1".to_owned()
    }

    // ─── Identity ─────────────────────────────────────────────────────

    #[tauri::command]
    pub fn keygen(state: tauri::State<AppState>) -> Result<NodeInfo, String> {
        let (sig_pub, sig_seed) = ed25519_generate();
        let (enc_pub, enc_priv) = x25519_generate();
        let node = LocalNode {
            sig: Keypair { pub_key: base64url::encode(&sig_pub),  priv_key: base64url::encode(&sig_seed) },
            enc: Keypair { pub_key: base64url::encode(&enc_pub),  priv_key: base64url::encode(&enc_priv) },
        };
        let info = node_info(&node)?;
        *state.node.lock().unwrap() = Some(node);
        Ok(info)
    }

    #[tauri::command]
    pub fn load_node(node_json: String, state: tauri::State<AppState>) -> Result<NodeInfo, String> {
        let node: LocalNode = serde_json::from_str(&node_json).map_err(to_err)?;
        let info = node_info(&node)?;
        *state.node.lock().unwrap() = Some(node);
        Ok(info)
    }

    #[tauri::command]
    pub fn get_node(state: tauri::State<AppState>) -> Option<NodeInfo> {
        state.node.lock().unwrap().as_ref().and_then(|n| node_info(n).ok())
    }

    // ─── Room lifecycle ───────────────────────────────────────────────

    #[tauri::command]
    pub fn create_room(
        name:       Option<String>,
        rules_json: Option<String>,
        state:      tauri::State<AppState>,
    ) -> Result<(String, String, String), String> {
        let guard = state.node.lock().unwrap();
        let node  = guard.as_ref().ok_or("No identity loaded")?;

        let (room_pub_bytes, room_seed) = ed25519_generate();
        let room = LocalRoom {
            sig: Keypair {
                pub_key:  base64url::encode(&room_pub_bytes),
                priv_key: base64url::encode(&room_seed),
            },
        };

        let rules: Option<Value> = rules_json.as_deref()
            .map(serde_json::from_str).transpose().map_err(to_err)?;

        let (_authored, committed) =
            make_room_create(node, &room, name.as_deref(), rules).map_err(to_err)?;

        // Persist the room key so local relays can auto-commit events for this room.
        state.rooms.lock().unwrap().insert(room.sig.pub_key.clone(), room.clone());

        let committed_json = serialize_env(&committed)?;
        let invite_uri    = discovery_encode_uri(&[committed]).map_err(to_err)?;
        Ok((room.sig.pub_key.clone(), committed_json, invite_uri))
    }

    #[tauri::command]
    pub fn join_room(room_pub: String, state: tauri::State<AppState>) -> Result<String, String> {
        let guard = state.node.lock().unwrap();
        let node  = guard.as_ref().ok_or("No identity loaded")?;
        serialize_env(&make_room_join(node, &room_pub, None).map_err(to_err)?)
    }

    // ─── Messaging ────────────────────────────────────────────────────

    #[tauri::command]
    pub fn sign_message(
        room_pub: String,
        channel:  i32,
        body:     String,
        state:    tauri::State<AppState>,
    ) -> Result<String, String> {
        let guard = state.node.lock().unwrap();
        let node  = guard.as_ref().ok_or("No identity loaded")?;
        serialize_env(&make_room_message(node, &room_pub, channel, &body, vec![]).map_err(to_err)?)
    }

    /// Wrap a depth-2 committed envelope in a depth-3 ack signed by the local node.
    /// The ack signals relays to stop active forwarding (terminal envelope, spec §6.2.3).
    #[tauri::command]
    pub fn ack_commit(committed_json: String, state: tauri::State<AppState>) -> Result<String, String> {
        let guard = state.node.lock().unwrap();
        let node  = guard.as_ref().ok_or("No identity loaded")?;
        let commit = parse_env_str(&committed_json)?;
        if depth(&commit) != 2 {
            return Err(format!("ack_commit: expected depth-2, got depth-{}", depth(&commit)));
        }
        serialize_env(&wrap_ack(commit, node).map_err(to_err)?)
    }

    // ─── Network / pool.exchange ──────────────────────────────────────

    #[tauri::command]
    pub async fn pool_exchange(
        relay_url:  String,
        offer:      Vec<String>,
        want_rooms: Vec<String>,
        after:      u64,
        state:      tauri::State<'_, AppState>,
    ) -> Result<Vec<String>, String> {
        let node: LocalNode = {
            let guard = state.node.lock().unwrap();
            guard.clone().unwrap_or_else(throwaway_node)
        };

        let offer_envelopes: Vec<Envelope> = offer
            .iter()
            .filter_map(|j| serde_json::from_str(j).ok())
            .collect();

        let config = PoolExchangeConfig {
            offer: offer_envelopes,
            want_rooms: want_rooms.into_iter()
                .map(|r| WantRoom { room_pub: r, after, limit: 200 })
                .collect(),
            timeout: Some(std::time::Duration::from_secs(30)),
        };

        // Use TlsVerify::AcceptAny for self-signed relay certs
        let envelopes = lib_pool_exchange(&relay_url, &node, config, TlsVerify::AcceptAny)
            .await
            .map_err(|e| e.to_string())?;

        Ok(envelopes.into_iter()
            .map(|env| serde_json::to_string(&env).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?)
    }

    #[tauri::command]
    pub async fn fetch_messages(
        relay_url: String,
        room_pub:  String,
        after:     u64,
        state:     tauri::State<'_, AppState>,
    ) -> Result<Vec<MessageItem>, String> {
        let envelopes = pool_exchange(relay_url, vec![], vec![room_pub], after, state).await?;
        Ok(envelopes.iter().filter_map(|j| {
            let env = parse_envelope(serde_json::from_str(j).ok()?).ok()?;
            message_item(&env)
        }).collect())
    }

    // ─── Relay config DB commands ─────────────────────────────────────

    /// Save a new relay config to the database (initially inactive).
    /// The `id` field must be a unique string generated by the frontend (e.g. UUID).
    #[tauri::command]
    pub fn relay_save_config(
        id:      String,
        proto:   String,
        host:    String,
        port:    u16,
        path:    Option<String>,
        direct:  bool,
        state:   tauri::State<AppState>,
    ) -> Result<RelayConfigRecord, String> {
        let record = RelayConfigRecord { id, proto, host, port, path, direct, active: false };
        with_db(&state, |db| db.save(&record))?;
        Ok(record)
    }

    /// Delete a saved relay config. Stops it first if it is currently running.
    #[tauri::command]
    pub fn relay_delete_config(id: String, state: tauri::State<AppState>) -> Result<(), String> {
        // Stop if running.
        {
            let mut relays = state.relays.lock().unwrap();
            if let Some(pos) = relays.iter().position(|r| r.config_id == id) {
                let entry = relays.remove(pos);
                let _ = entry.stop_tx.send(());
            }
        }
        with_db(&state, |db| db.delete(&id))
    }

    /// Return all saved relay configs from the database.
    #[tauri::command]
    pub fn relay_list_configs(state: tauri::State<AppState>) -> Result<Vec<RelayConfigRecord>, String> {
        with_db(&state, |db| db.list())
    }

    // ─── Relay runtime commands ───────────────────────────────────────

    /// Start a saved relay by its config ID.
    /// Loads the config from the DB, starts the relay thread, marks the config as active.
    /// Uses the loaded node identity (stable across restarts) and derives TLS cert deterministically.
    #[tauri::command]
    pub fn relay_start(config_id: String, state: tauri::State<AppState>) -> Result<RelayInfo, String> {
        let cfg = with_db(&state, |db| {
            db.list()?.into_iter()
                .find(|r| r.id == config_id)
                .ok_or_else(|| format!("relay config {config_id} not found"))
        })?;

        // Get the loaded node identity (must be loaded via load_node or keygen first).
        let node = {
            let guard = state.node.lock().unwrap();
            guard.clone().ok_or("No identity loaded — call load_node or keygen first")?
        };

        // Probe port availability.
        std::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port))
            .map_err(|e| format!("port {} unavailable: {e}", cfg.port))?;

        // For TLS protos (wss, https) generate a deterministic self-signed cert from the node's Ed25519 seed.
        let needs_tls = cfg.proto == "wss" || cfg.proto == "https";
        let (tls_acceptor, cert_fp) = if needs_tls {
            let seed_bytes = base64url::decode(&node.sig.priv_key)
                .map_err(|e| format!("invalid node seed: {e}"))?;
            let seed: [u8; 32] = seed_bytes.as_slice().try_into()
                .map_err(|_| "node seed must be 32 bytes".to_owned())?;
            match make_self_signed_acceptor_from_key(&seed, &cfg.host) {
                Ok((acc, _cert_der, fp)) => (Some(std::sync::Arc::new(acc)), Some(fp)),
                Err(e) => return Err(format!("TLS setup failed: {e}")),
            }
        } else {
            (None, None)
        };

        // Build endpoint descriptor for the invite URI.
        let mut extra: std::collections::HashMap<String, serde_json::Value> = Default::default();
        if let Some(fp) = cert_fp {
            extra.insert("cert_fp".to_owned(), serde_json::json!(fp));
        }
        let endpoint_desc = EndpointDescriptor {
            proto:   cfg.proto.clone(),
            host:    Some(cfg.host.clone()),
            port:    Some(cfg.port),
            path:    cfg.path.clone(),
            direct:  Some(cfg.direct),
            extra,
        };
        let node_meta  = make_node_meta(&node, vec![endpoint_desc]).map_err(to_err)?;
        let invite_uri = discovery_encode_uri(&[node_meta]).map_err(to_err)?;

        let bind_addr: SocketAddr = format!("0.0.0.0:{}", cfg.port).parse().map_err(to_err)?;
        let transport_ep = EndpointConfig {
            addr:  bind_addr,
            proto: cfg.proto.clone(),
            path:  cfg.path.clone(),
            tls:   tls_acceptor,
        };

        // Snapshot room keys the node currently holds — relay uses these to auto-commit.
        let rooms_snapshot: HashMap<String, LocalRoom> = state.rooms.lock().unwrap().clone();

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let metrics        = Arc::new(RelayMetrics::new());
        let metrics_thread = Arc::clone(&metrics);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().expect("relay runtime");
            rt.block_on(async move {
                let pool = match Pool::memory() {
                    Ok(p)  => Arc::new(p),
                    Err(e) => { eprintln!("[toloo-app] relay pool init failed: {e}"); return; }
                };
                let config = Arc::new(RelayConfig {
                    node,
                    endpoints: vec![transport_ep],
                    pool,
                    rooms:     rooms_snapshot,
                    metrics:   Some(metrics_thread),
                });
                tokio::select! {
                    res = run_relay(config) => {
                        if let Err(e) = res { eprintln!("[toloo-app] relay error: {e}"); }
                    }
                    _ = stop_rx => {}
                }
            });
        });

        let runtime_id = {
            let mut next = state.relay_id_next.lock().unwrap();
            let id = *next; *next += 1; id
        };

        let entry = LocalRelayEntry {
            runtime_id,
            config_id: cfg.id.clone(),
            stop_tx,
            invite_uri,
            proto: cfg.proto.clone(),
            host:  cfg.host.clone(),
            port:  cfg.port,
            metrics,
        };
        let info = relay_info(&entry);
        state.relays.lock().unwrap().push(entry);

        // Mark as active in DB.
        let _ = with_db(&state, |db| db.set_active(&cfg.id, true));

        Ok(info)
    }

    /// Stop a running relay by its runtime_id. Marks the config as inactive in the DB.
    #[tauri::command]
    pub fn relay_stop(runtime_id: u32, state: tauri::State<AppState>) -> Result<(), String> {
        let config_id = {
            let mut relays = state.relays.lock().unwrap();
            if let Some(pos) = relays.iter().position(|r| r.runtime_id == runtime_id) {
                let entry = relays.remove(pos);
                let id = entry.config_id.clone();
                let _ = entry.stop_tx.send(());
                Some(id)
            } else { None }
        };
        if let Some(id) = config_id {
            let _ = with_db(&state, |db| db.set_active(&id, false));
        }
        Ok(())
    }

    /// Return all running relays with live metrics.
    #[tauri::command]
    pub fn relay_list(state: tauri::State<AppState>) -> Vec<RelayInfo> {
        state.relays.lock().unwrap().iter().map(relay_info).collect()
    }

    /// Return the detected LAN IP (for pre-filling the relay host field in the UI).
    #[tauri::command]
    pub fn detect_lan_ip_cmd() -> String {
        detect_lan_ip()
    }

    // ─── Envelope utilities ───────────────────────────────────────────

    #[tauri::command]
    pub fn verify_envelope(envelope_json: String) -> Result<bool, String> {
        verify_chain(&parse_env_str(&envelope_json)?).map(|_| true).map_err(to_err)
    }

    #[tauri::command]
    pub fn envelope_eid(envelope_json: String) -> Result<String, String> {
        Ok(eid(&parse_env_str(&envelope_json)?))
    }

    // ─── Discovery ────────────────────────────────────────────────────

    #[tauri::command]
    pub fn encode_uri(envelope_jsons: Vec<String>) -> Result<String, String> {
        let envs: Result<Vec<Envelope>, String> =
            envelope_jsons.iter().map(|j| parse_env_str(j)).collect();
        discovery_encode_uri(&envs?).map_err(to_err)
    }

    #[tauri::command]
    pub fn encode_file(envelope_jsons: Vec<String>) -> Result<String, String> {
        let envs: Result<Vec<Envelope>, String> =
            envelope_jsons.iter().map(|j| parse_env_str(j)).collect();
        discovery_encode_file(&envs?).map_err(to_err)
    }

    #[tauri::command]
    pub fn decode(input: String) -> Result<Vec<String>, String> {
        discovery_decode(&input).map_err(to_err)?
            .iter().map(serialize_env).collect()
    }

    #[tauri::command]
    pub fn decode_invite(input: String) -> Result<InviteInfo, String> {
        let envelopes = discovery_decode(&input).map_err(to_err)?;

        let room_env = envelopes.iter().find(|e| {
            let t = &e.d.t;
            t == "commit" || innermost(e).d.t == "room.create"
        }).ok_or("no room.create envelope found in invite")?;

        let room_pub = if room_env.d.t == "commit" {
            room_env.d.n.clone()
        } else {
            room_env.d.r.clone().ok_or("room.create missing r field")?
        };

        let inner     = innermost(room_env);
        let creator   = inner.d.n.clone();
        let room_name = inner.d.c.as_ref()
            .and_then(|c| c.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        let invite_uri     = discovery_encode_uri(&envelopes).map_err(to_err)?;
        let envelope_jsons = envelopes.iter().map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>().map_err(to_err)?;

        Ok(InviteInfo { room_pub, room_name, creator, invite_uri, envelope_jsons })
    }

    // ─── Encrypted identity ───────────────────────────────────────────

    #[tauri::command]
    pub fn encrypt_identity(passphrase: String, state: tauri::State<AppState>) -> Result<String, String> {
        let guard = state.node.lock().unwrap();
        let node = guard.as_ref().ok_or("No identity loaded")?;
        let node_json = serde_json::to_string(node).map_err(to_err)?;
        let encrypted = keystore::encrypt_key(node_json.as_bytes(), &passphrase).map_err(to_err)?;
        Ok(encrypted.blob)
    }

    #[tauri::command]
    pub fn load_encrypted_identity(
        encrypted_blob: String,
        passphrase:     String,
        state:          tauri::State<AppState>,
    ) -> Result<NodeInfo, String> {
        let encrypted = keystore::EncryptedKey { blob: encrypted_blob };
        let decrypted = keystore::decrypt_key(&encrypted, &passphrase).map_err(to_err)?;
        let node_json = std::str::from_utf8(&decrypted).map_err(to_err)?;
        let node: LocalNode = serde_json::from_str(node_json).map_err(to_err)?;
        let info = node_info(&node)?;
        *state.node.lock().unwrap() = Some(node);
        Ok(info)
    }

    // ─── Attestations ─────────────────────────────────────────────────

    #[tauri::command]
    pub fn create_attestation(
        target_node: String,
        level:       String,
        reason:      Option<String>,
        state:       tauri::State<AppState>,
    ) -> Result<String, String> {
        let guard = state.node.lock().unwrap();
        let node = guard.as_ref().ok_or("No identity loaded")?;
        let env = make_side_attestation(node, &target_node, &level, reason.as_deref()).map_err(to_err)?;
        serialize_env(&env)
    }

    // ─── Flags ────────────────────────────────────────────────────────

    #[tauri::command]
    pub fn create_flag(
        room_pub:   String,
        target_eid: String,
        category:   String,
        reason:     Option<String>,
        state:      tauri::State<AppState>,
    ) -> Result<String, String> {
        let guard = state.node.lock().unwrap();
        let node = guard.as_ref().ok_or("No identity loaded")?;
        let env = make_room_flag(node, &room_pub, &target_eid, &category, reason.as_deref()).map_err(to_err)?;
        serialize_env(&env)
    }

    // ─── Local moderation (blocklist) ─────────────────────────────────

    #[tauri::command]
    pub fn block_node_cmd(
        node_pub: String,
        reason:   Option<String>,
        state:    tauri::State<AppState>,
    ) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(to_err)?
            .as_millis() as u64;
        with_db(&state, |db| db.block_node(&node_pub, "block", reason.as_deref(), now))
    }

    #[tauri::command]
    pub fn unblock_node_cmd(node_pub: String, state: tauri::State<AppState>) -> Result<(), String> {
        with_db(&state, |db| db.unblock_node(&node_pub).map(|_| ()))
    }

    #[tauri::command]
    pub fn get_blocklist_cmd(state: tauri::State<AppState>) -> Result<Vec<BlocklistItem>, String> {
        with_db(&state, |db| {
            db.get_blocklist().map(|rows| rows.into_iter().map(|(node_pub, kind, reason, blocked_at)| {
                BlocklistItem { node_pub, kind, reason, blocked_at }
            }).collect())
        })
    }

    #[tauri::command]
    pub fn is_blocked_cmd(node_pub: String, state: tauri::State<AppState>) -> Result<bool, String> {
        with_db(&state, |db| db.is_blocked(&node_pub))
    }
}

// ══════════════════════════════════════════════════════════════════════
// Entry point
// ══════════════════════════════════════════════════════════════════════

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            node:          Mutex::new(None),
            rooms:         Mutex::new(HashMap::new()),
            relays:        Mutex::new(Vec::new()),
            relay_id_next: Mutex::new(0),
            db:            Mutex::new(None),
        })
        .setup(|app| {
            use tauri::Manager;
            let data_dir = app.path().app_data_dir()?;
            match AppDb::open(&data_dir.join("toloo-relays.db")) {
                Ok(db) => {
                    let state: tauri::State<AppState> = app.state();
                    *state.db.lock().unwrap() = Some(db);
                }
                Err(e) => eprintln!("[toloo-app] relay DB init failed: {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::keygen,
            commands::load_node,
            commands::get_node,
            commands::create_room,
            commands::join_room,
            commands::sign_message,
            commands::ack_commit,
            commands::pool_exchange,
            commands::fetch_messages,
            commands::verify_envelope,
            commands::envelope_eid,
            commands::encode_uri,
            commands::encode_file,
            commands::decode,
            commands::decode_invite,
            commands::relay_save_config,
            commands::relay_delete_config,
            commands::relay_list_configs,
            commands::relay_start,
            commands::relay_stop,
            commands::relay_list,
            commands::detect_lan_ip_cmd,
            commands::encrypt_identity,
            commands::load_encrypted_identity,
            commands::create_attestation,
            commands::create_flag,
            commands::block_node_cmd,
            commands::unblock_node_cmd,
            commands::get_blocklist_cmd,
            commands::is_blocked_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running toloo app");
}
