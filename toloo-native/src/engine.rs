use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use toloo_lib::base64url;
use toloo_lib::crypto::{
    ed25519_generate, ed25519_pub_from_seed, x25519_generate, x25519_pub_from_seed,
};
use toloo_lib::discovery;
use toloo_lib::envelope::{depth, innermost, wrap_commit};
use toloo_lib::events::{
    make_room_create, make_room_delete, make_room_flag, make_room_join, make_room_leave,
    make_room_message, make_room_update, make_side_attestation,
};
use toloo_lib::ids::eid;
use toloo_lib::keystore::{self, EncryptedKey};
use toloo_lib::pool::{Pool, PoolConfig};
use toloo_lib::transport::client::{pool_exchange, PoolExchangeConfig, TlsVerify, WantRoom};
use toloo_lib::transport::server::{run_relay, EndpointConfig, RelayConfig, RelayMetrics};
use toloo_lib::types::{Envelope, Keypair, LocalNode, LocalRoom};

const SYNC_INTERVAL: Duration = Duration::from_secs(10);
const MAX_VISIBLE_MESSAGES: usize = 300;

#[derive(Debug)]
pub enum Command {
    CreateIdentity,
    RestoreIdentity {
        sig_seed: String,
        enc_priv: String,
    },
    LoadIdentityJson(String),
    ExportIdentity,
    ExportEncryptedIdentity(String),
    LoadEncryptedIdentity {
        blob: String,
        passphrase: String,
    },
    RemoveIdentity,
    AddRelay(String),
    RemoveRelay(String),
    CreateRoom(String),
    JoinRoom(String),
    SelectRoom(String),
    RenameRoom {
        room_id: String,
        name: String,
    },
    LeaveRoom(String),
    RemoveRoom(String),
    ImportDiscovery(String),
    ExportInvite(String),
    ExportMessages(String),
    DeleteMessage {
        room_id: String,
        target: String,
    },
    FlagMessage {
        room_id: String,
        target: String,
        category: String,
    },
    AttestNode {
        target: String,
        level: String,
    },
    BlockNode {
        node_pub: String,
        kind: String,
        reason: String,
    },
    UnblockNode(String),
    SetRtl(bool),
    SetNotifications(bool),
    SetSearch(String),
    StartHost {
        port: u16,
        proto: String,
    },
    StopHost,
    Sync,
    SyncRoom(String),
    SendMessage(String),
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub has_identity: bool,
    pub identity_label: String,
    pub identity_initial: String,
    pub identity_sig_pub: String,
    pub identity_enc_pub: String,
    pub status: String,
    pub network_label: String,
    pub active_room: String,
    pub active_room_id: String,
    pub room_key: String,
    pub room_initial: String,
    pub room_has_key: bool,
    pub members_label: String,
    pub rtl: bool,
    pub notifications_enabled: bool,
    pub warming: bool,
    pub export_text: String,
    pub export_kind: String,
    pub relays: Vec<RelayView>,
    pub blocked: Vec<BlockedView>,
    pub host_running: bool,
    pub host_address: String,
    pub host_connections: i32,
    pub host_port: String,
    pub rooms: Vec<RoomView>,
    pub messages: Vec<MessageView>,
}

#[derive(Debug, Clone)]
pub struct RelayView {
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct BlockedView {
    pub node_pub: String,
    pub short: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RoomView {
    pub room_id: String,
    pub display_name: String,
    pub initial: String,
    pub selected: bool,
    pub has_key: bool,
    pub preview: String,
    pub time_label: String,
    pub unread: i32,
    pub member_count: i32,
}

#[derive(Debug, Clone)]
pub struct MessageView {
    pub eid: String,
    pub author: String,
    pub author_id: String,
    pub author_initial: String,
    pub body: String,
    pub clock: String,
    pub mine: bool,
    pub committed: bool,
    pub status: String,
    pub day_label: String,
    pub new_day: bool,
    pub grouped: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            has_identity: false,
            identity_label: "No identity".to_owned(),
            identity_initial: "?".to_owned(),
            identity_sig_pub: String::new(),
            identity_enc_pub: String::new(),
            status: String::new(),
            network_label: "No relays".to_owned(),
            active_room: String::new(),
            active_room_id: String::new(),
            room_key: String::new(),
            room_initial: "?".to_owned(),
            room_has_key: false,
            members_label: String::new(),
            rtl: false,
            notifications_enabled: false,
            warming: false,
            export_text: String::new(),
            export_kind: String::new(),
            relays: Vec::new(),
            blocked: Vec::new(),
            host_running: false,
            host_address: String::new(),
            host_connections: 0,
            host_port: "17701".to_owned(),
            rooms: Vec::new(),
            messages: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    identity: Option<LocalNode>,
    relays: Vec<String>,
    rooms: Vec<RoomConfig>,
    active: Option<String>,
    #[serde(default)]
    rtl: bool,
    #[serde(default)]
    notifications: bool,
    #[serde(default)]
    read_marks: HashMap<String, u64>,
    #[serde(default)]
    host: Option<HostConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostConfig {
    port: u16,
    proto: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoomConfig {
    pub_key: String,
    name: String,
    room_identity: Option<LocalRoom>,
    room_create: Option<Envelope>,
}

struct HostHandle {
    stop: tokio::sync::oneshot::Sender<()>,
    metrics: Arc<RelayMetrics>,
    port: u16,
    proto: String,
}

struct Engine {
    data_dir: PathBuf,
    config: Config,
    pool: Arc<Pool>,
    runtime: tokio::runtime::Runtime,
    status: String,
    export_text: String,
    export_kind: String,
    search: String,
    last_outgoing_ts: u64,
    host: Option<HostHandle>,
}

fn config_snapshot(config: &Config, status: &str) -> Snapshot {
    let node_pub = config
        .identity
        .as_ref()
        .map(|node| node.sig.pub_key.clone());
    let active = config
        .active
        .as_deref()
        .and_then(|active| config.rooms.iter().find(|room| room.pub_key == active));
    let rooms = config
        .rooms
        .iter()
        .map(|room| RoomView {
            room_id: room.pub_key.clone(),
            display_name: room.name.clone(),
            initial: initial(&room.name),
            selected: active
                .map(|active| active.pub_key == room.pub_key)
                .unwrap_or(false),
            has_key: room.room_identity.is_some(),
            preview: String::new(),
            time_label: String::new(),
            unread: 0,
            member_count: 0,
        })
        .collect();
    let network_label = match config.relays.len() {
        0 => "No relays".to_owned(),
        1 => "1 relay".to_owned(),
        count => format!("{count} relays"),
    };
    Snapshot {
        has_identity: config.identity.is_some(),
        identity_label: node_pub
            .as_deref()
            .map(short_key)
            .unwrap_or_else(|| "No identity".to_owned()),
        identity_initial: node_pub
            .as_deref()
            .map(initial)
            .unwrap_or_else(|| "?".to_owned()),
        identity_sig_pub: node_pub.clone().unwrap_or_default(),
        identity_enc_pub: config
            .identity
            .as_ref()
            .map(|node| node.enc.pub_key.clone())
            .unwrap_or_default(),
        status: status.to_owned(),
        network_label,
        active_room: active.map(|room| room.name.clone()).unwrap_or_default(),
        active_room_id: active.map(|room| room.pub_key.clone()).unwrap_or_default(),
        room_key: active
            .map(|room| short_key(&room.pub_key))
            .unwrap_or_default(),
        room_initial: active
            .map(|room| initial(&room.name))
            .unwrap_or_else(|| "?".to_owned()),
        room_has_key: active
            .map(|room| room.room_identity.is_some())
            .unwrap_or(false),
        members_label: String::new(),
        rtl: config.rtl,
        notifications_enabled: config.notifications,
        warming: true,
        export_text: String::new(),
        export_kind: String::new(),
        relays: config
            .relays
            .iter()
            .map(|url| RelayView { url: url.clone() })
            .collect(),
        blocked: Vec::new(),
        host_running: false,
        host_address: config
            .host
            .as_ref()
            .map(|host| format!("{}://0.0.0.0:{}", host.proto, host.port))
            .unwrap_or_default(),
        host_connections: 0,
        host_port: config
            .host
            .as_ref()
            .map(|host| host.port.to_string())
            .unwrap_or_else(|| "17701".to_owned()),
        rooms,
        messages: Vec::new(),
    }
}

impl Engine {
    fn load(data_dir: PathBuf, updates: Option<&Sender<Snapshot>>) -> Result<Self> {
        #[cfg(target_os = "android")]
        let load_started = std::time::Instant::now();
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("creating {}", data_dir.display()))?;
        let config_path = data_dir.join("config.json");
        let config = if config_path.exists() {
            serde_json::from_slice(&std::fs::read(&config_path)?)
                .with_context(|| format!("parsing {}", config_path.display()))?
        } else {
            Config::default()
        };
        #[cfg(target_os = "android")]
        eprintln!(
            "toloo: config loaded at {}ms",
            load_started.elapsed().as_millis()
        );
        if let Some(updates) = updates {
            let _ = updates.send(config_snapshot(&config, "Loading history…"));
        }
        let pool_base = data_dir.join("pool");
        let pool = Arc::new(Pool::open(
            pool_base
                .to_str()
                .context("Toloo storage path is not valid UTF-8")?,
            PoolConfig::default(),
        )?);
        #[cfg(target_os = "android")]
        eprintln!(
            "toloo: pool opened at {}ms",
            load_started.elapsed().as_millis()
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut engine = Self {
            data_dir,
            config,
            pool,
            runtime,
            status: "Ready".to_owned(),
            export_text: String::new(),
            export_kind: String::new(),
            search: String::new(),
            last_outgoing_ts: 0,
            host: None,
        };
        if engine.config.active.is_none() {
            engine.config.active = engine.config.rooms.first().map(|room| room.pub_key.clone());
        }
        Ok(engine)
    }

    fn save_config(&self) -> Result<()> {
        let path = self.data_dir.join("config.json");
        let temp = self.data_dir.join("config.json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&self.config)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }

    fn node(&self) -> Result<&LocalNode> {
        self.config
            .identity
            .as_ref()
            .ok_or_else(|| anyhow!("Create an identity first"))
    }

    fn active_room(&self) -> Option<&RoomConfig> {
        let active = self.config.active.as_deref()?;
        self.config.rooms.iter().find(|room| room.pub_key == active)
    }

    fn handle(&mut self, command: Command) -> Result<()> {
        match command {
            Command::CreateIdentity => self.create_identity(),
            Command::RestoreIdentity { sig_seed, enc_priv } => {
                self.restore_identity(&sig_seed, &enc_priv)
            }
            Command::LoadIdentityJson(json) => self.load_identity_json(&json),
            Command::ExportIdentity => self.export_identity(),
            Command::ExportEncryptedIdentity(passphrase) => {
                self.export_encrypted_identity(&passphrase)
            }
            Command::LoadEncryptedIdentity { blob, passphrase } => {
                self.load_encrypted_identity(&blob, &passphrase)
            }
            Command::RemoveIdentity => self.remove_identity(),
            Command::AddRelay(url) => self.add_relay(&url),
            Command::RemoveRelay(url) => self.remove_relay(&url),
            Command::CreateRoom(name) => self.create_room(&name),
            Command::JoinRoom(input) => self.join_room(&input),
            Command::SelectRoom(room_id) => self.select_room(&room_id),
            Command::RenameRoom { room_id, name } => self.rename_room(&room_id, &name),
            Command::LeaveRoom(room_id) => self.leave_room(&room_id),
            Command::RemoveRoom(room_id) => self.remove_room(&room_id),
            Command::ImportDiscovery(input) => self.import_discovery(&input),
            Command::ExportInvite(room_id) => self.export_invite(&room_id),
            Command::ExportMessages(room_id) => self.export_messages(&room_id),
            Command::DeleteMessage { room_id, target } => self.delete_message(&room_id, &target),
            Command::FlagMessage {
                room_id,
                target,
                category,
            } => self.flag_message(&room_id, &target, &category),
            Command::AttestNode { target, level } => self.attest_node(&target, &level),
            Command::BlockNode {
                node_pub,
                kind,
                reason,
            } => self.block_node(&node_pub, &kind, &reason),
            Command::UnblockNode(node_pub) => self.unblock_node(&node_pub),
            Command::SetRtl(enabled) => self.set_rtl(enabled),
            Command::SetNotifications(enabled) => self.set_notifications(enabled),
            Command::SetSearch(query) => {
                self.search = query.trim().to_lowercase();
                Ok(())
            }
            Command::StartHost { port, proto } => self.start_host(port, &proto),
            Command::StopHost => self.stop_host(),
            Command::Sync => self.sync_active(),
            Command::SyncRoom(room_id) => self.sync_room(&room_id),
            Command::SendMessage(body) => self.send_message(&body),
        }
    }

    fn create_identity(&mut self) -> Result<()> {
        if self.config.identity.is_some() {
            bail!("An identity is already loaded");
        }
        let (sig_pub, sig_seed) = ed25519_generate();
        let (enc_pub, enc_priv) = x25519_generate();
        self.config.identity = Some(LocalNode {
            sig: Keypair {
                pub_key: base64url::encode(&sig_pub),
                priv_key: base64url::encode(&sig_seed),
            },
            enc: Keypair {
                pub_key: base64url::encode(&enc_pub),
                priv_key: base64url::encode(&enc_priv),
            },
        });
        self.save_config()?;
        self.status = "Identity created".to_owned();
        Ok(())
    }

    fn restore_identity(&mut self, sig_seed: &str, enc_priv: &str) -> Result<()> {
        let sig_seed_bytes = base64url::decode(sig_seed.trim())?;
        let sig_pub = ed25519_pub_from_seed(&sig_seed_bytes)?;
        let enc_seed: [u8; 32] = base64url::decode(enc_priv.trim())?
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("Encryption key must be 32 bytes"))?;
        let enc_pub = x25519_pub_from_seed(&enc_seed);
        self.install_identity(LocalNode {
            sig: Keypair {
                pub_key: base64url::encode(&sig_pub),
                priv_key: sig_seed.trim().to_owned(),
            },
            enc: Keypair {
                pub_key: base64url::encode(&enc_pub),
                priv_key: enc_priv.trim().to_owned(),
            },
        })?;
        self.status = "Identity restored".to_owned();
        Ok(())
    }

    fn install_identity(&mut self, node: LocalNode) -> Result<()> {
        let sig_seed = base64url::decode(node.sig.priv_key.trim())?;
        let sig_pub = base64url::encode(&ed25519_pub_from_seed(&sig_seed)?);
        if sig_pub != node.sig.pub_key {
            bail!("Signing public key does not match the private seed");
        }
        let enc_seed: [u8; 32] = base64url::decode(node.enc.priv_key.trim())?
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("Encryption key must be 32 bytes"))?;
        if base64url::encode(&x25519_pub_from_seed(&enc_seed)) != node.enc.pub_key {
            bail!("Encryption public key does not match the private key");
        }
        self.config.identity = Some(node);
        self.save_config()?;
        Ok(())
    }

    fn load_identity_json(&mut self, json: &str) -> Result<()> {
        let node: LocalNode = serde_json::from_str(json.trim())
            .context("Identity file is not a valid Toloo identity")?;
        self.install_identity(node)?;
        self.status = "Identity imported".to_owned();
        Ok(())
    }

    fn export_identity(&mut self) -> Result<()> {
        let node = self.node()?.clone();
        self.export_text = serde_json::to_string_pretty(&node)?;
        self.export_kind = "identity".to_owned();
        self.status = "Identity exported — keep this file secret".to_owned();
        Ok(())
    }

    fn export_encrypted_identity(&mut self, passphrase: &str) -> Result<()> {
        if passphrase.trim().is_empty() {
            bail!("Enter a passphrase");
        }
        let node = self.node()?.clone();
        let plaintext = serde_json::to_vec(&node)?;
        let encrypted = keystore::encrypt_key(&plaintext, passphrase)?;
        self.export_text = encrypted.blob;
        self.export_kind = "encrypted-identity".to_owned();
        self.status = "Encrypted identity exported".to_owned();
        Ok(())
    }

    fn load_encrypted_identity(&mut self, blob: &str, passphrase: &str) -> Result<()> {
        let encrypted = EncryptedKey {
            blob: blob.trim().to_owned(),
        };
        let plaintext = keystore::decrypt_key(&encrypted, passphrase)
            .context("Wrong passphrase or corrupted identity file")?;
        let node: LocalNode =
            serde_json::from_slice(&plaintext).context("Decrypted data is not a Toloo identity")?;
        self.install_identity(node)?;
        self.status = "Identity unlocked".to_owned();
        Ok(())
    }

    fn remove_identity(&mut self) -> Result<()> {
        self.config.identity = None;
        self.save_config()?;
        self.status = "Identity removed".to_owned();
        Ok(())
    }

    fn add_relay(&mut self, url: &str) -> Result<()> {
        let url = url.trim();
        if !["ws://", "wss://", "http://", "https://"]
            .iter()
            .any(|prefix| url.starts_with(prefix))
        {
            bail!("Relay URL must start with ws://, wss://, http://, or https://");
        }
        if !self.config.relays.iter().any(|relay| relay == url) {
            self.config.relays.push(url.to_owned());
            self.save_config()?;
        }
        self.status = "Relay added".to_owned();
        Ok(())
    }

    fn remove_relay(&mut self, url: &str) -> Result<()> {
        let before = self.config.relays.len();
        self.config.relays.retain(|relay| relay != url);
        if self.config.relays.len() == before {
            bail!("Unknown relay");
        }
        self.save_config()?;
        self.status = "Relay removed".to_owned();
        Ok(())
    }

    fn create_room(&mut self, name: &str) -> Result<()> {
        let node = self.node()?.clone();
        let (room_pub, room_seed) = ed25519_generate();
        let room = LocalRoom {
            sig: Keypair {
                pub_key: base64url::encode(&room_pub),
                priv_key: base64url::encode(&room_seed),
            },
        };
        let rules = json!([
            {"t": "join", "allow": "*"},
            {"t": "post", "allow": "*"}
        ]);
        let (_, committed) = make_room_create(
            &node,
            &room,
            Some(if name.trim().is_empty() {
                "Untitled room"
            } else {
                name.trim()
            }),
            Some(rules),
        )?;
        self.pool.put(&committed)?;
        let room_config = RoomConfig {
            pub_key: room.sig.pub_key.clone(),
            name: if name.trim().is_empty() {
                "Untitled room".to_owned()
            } else {
                name.trim().to_owned()
            },
            room_identity: Some(room.clone()),
            room_create: Some(committed.clone()),
        };
        self.config.rooms.push(room_config);
        self.config.active = Some(room.sig.pub_key.clone());
        self.save_config()?;

        match self.exchange(vec![committed], &room.sig.pub_key) {
            Ok(_) => self.status = "Room created and published".to_owned(),
            Err(error) => self.status = format!("Room saved locally; publish failed: {error:#}"),
        }
        Ok(())
    }

    fn join_room(&mut self, input: &str) -> Result<()> {
        let node = self.node()?.clone();
        let text = input.trim();
        if text.is_empty() {
            bail!("Paste an invite or room public key");
        }
        let mut room_pub = None;
        let mut name = None;
        let mut room_create = None;
        if text.starts_with("toloo:") || text.contains('\n') {
            let envelopes = discovery::decode(text)?;
            for envelope in envelopes {
                let inner = innermost(&envelope);
                if inner.d.t == "room.create" {
                    room_pub = inner.d.r.clone();
                    name = inner
                        .d
                        .c
                        .as_ref()
                        .and_then(|content| content.get("name"))
                        .and_then(|value| value.as_str())
                        .map(str::to_owned);
                    room_create = Some(envelope);
                    break;
                }
            }
            let Some(room_pub) = room_pub else {
                bail!("Invite does not contain a room.create");
            };
            let room_pub = room_pub;
            self.add_room_if_missing(&room_pub, name, room_create);
        } else {
            let valid_key = text.len() == 43
                && base64url::decode(text)
                    .map(|bytes| bytes.len() == 32)
                    .unwrap_or(false);
            if !valid_key {
                bail!("Room public key must be 43 base64url characters");
            }
            self.add_room_if_missing(text, None, None);
        }
        let active = self
            .config
            .active
            .clone()
            .ok_or_else(|| anyhow!("Room was not selected"))?;
        let join = make_room_join(&node, &active, None)?;
        self.pool.put(&join)?;
        self.save_config()?;
        let room = self
            .active_room()
            .cloned()
            .ok_or_else(|| anyhow!("Room was not selected"))?;
        let offers = room
            .room_create
            .clone()
            .map(|create| vec![create, join.clone()])
            .unwrap_or_else(|| vec![join]);
        match self.exchange(offers, &active) {
            Ok(_) => self.status = "Join request sent".to_owned(),
            Err(error) => self.status = format!("Join saved locally: {error:#}"),
        }
        Ok(())
    }

    fn add_room_if_missing(
        &mut self,
        room_pub: &str,
        name: Option<String>,
        room_create: Option<Envelope>,
    ) {
        if let Some(existing) = self
            .config
            .rooms
            .iter_mut()
            .find(|room| room.pub_key == room_pub)
        {
            if existing.room_create.is_none() {
                existing.room_create = room_create;
            }
            if let Some(name) = name {
                existing.name = name;
            }
            self.config.active = Some(room_pub.to_owned());
            return;
        }
        self.config.rooms.push(RoomConfig {
            pub_key: room_pub.to_owned(),
            name: name.unwrap_or_else(|| short_key(room_pub)),
            room_identity: None,
            room_create,
        });
        self.config.active = Some(room_pub.to_owned());
    }

    fn select_room(&mut self, room_id: &str) -> Result<()> {
        if !self.config.rooms.iter().any(|room| room.pub_key == room_id) {
            bail!("Unknown room");
        }
        self.config.active = Some(room_id.to_owned());
        self.config.read_marks.insert(room_id.to_owned(), now_ms());
        self.save_config()?;
        Ok(())
    }

    fn rename_room(&mut self, room_id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("Room name cannot be empty");
        }
        let node = self.node()?.clone();
        let room = self
            .config
            .rooms
            .iter()
            .find(|room| room.pub_key == room_id)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown room"))?;
        let room_identity = room
            .room_identity
            .clone()
            .ok_or_else(|| anyhow!("Only the room owner can rename this room"))?;
        let update = make_room_update(&node, &room_identity, Some(name), None)?;
        let committed = wrap_commit(update, &room_identity, now_ms())?;
        self.pool.put(&committed)?;
        if let Some(room) = self
            .config
            .rooms
            .iter_mut()
            .find(|room| room.pub_key == room_id)
        {
            room.name = name.to_owned();
        }
        self.save_config()?;
        if self.config.relays.is_empty() {
            self.status = "Room renamed locally".to_owned();
        } else {
            match self.exchange(vec![committed], room_id) {
                Ok(_) => self.status = "Room renamed".to_owned(),
                Err(error) => self.status = format!("Room renamed locally: {error:#}"),
            }
        }
        Ok(())
    }

    fn leave_room(&mut self, room_id: &str) -> Result<()> {
        let node = self.node()?.clone();
        let room = self
            .config
            .rooms
            .iter()
            .find(|room| room.pub_key == room_id)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown room"))?;
        let leave = make_room_leave(&node, &room.pub_key)?;
        let outgoing = if let Some(room_identity) = &room.room_identity {
            wrap_commit(leave, room_identity, now_ms())?
        } else {
            leave
        };
        self.pool.put(&outgoing)?;
        self.config.rooms.retain(|room| room.pub_key != room_id);
        self.config.read_marks.remove(room_id);
        if self.config.active.as_deref() == Some(room_id) {
            self.config.active = self.config.rooms.first().map(|room| room.pub_key.clone());
        }
        self.save_config()?;
        if self.config.relays.is_empty() {
            self.status = "Left room".to_owned();
            return Ok(());
        }
        let offers = room
            .room_create
            .clone()
            .map(|create| vec![create, outgoing.clone()])
            .unwrap_or_else(|| vec![outgoing]);
        match self.exchange(offers, room_id) {
            Ok(_) => self.status = "Left room".to_owned(),
            Err(error) => self.status = format!("Left room locally: {error:#}"),
        }
        Ok(())
    }

    fn remove_room(&mut self, room_id: &str) -> Result<()> {
        let before = self.config.rooms.len();
        self.config.rooms.retain(|room| room.pub_key != room_id);
        if self.config.rooms.len() == before {
            bail!("Unknown room");
        }
        self.config.read_marks.remove(room_id);
        if self.config.active.as_deref() == Some(room_id) {
            self.config.active = self.config.rooms.first().map(|room| room.pub_key.clone());
        }
        self.save_config()?;
        self.status = "Room removed from this device".to_owned();
        Ok(())
    }

    fn import_discovery(&mut self, input: &str) -> Result<()> {
        let envelopes = discovery::decode(input)?;
        let mut imported = 0usize;
        let mut rooms_added = 0usize;
        let mut relays_added = 0usize;
        for envelope in &envelopes {
            if self.pool.put(envelope).is_ok() {
                imported += 1;
            }
            let inner = innermost(envelope);
            match inner.d.t.as_str() {
                "room.create" => {
                    let room_pub = if envelope.d.t == "commit" {
                        envelope.d.n.clone()
                    } else {
                        inner.d.r.clone().unwrap_or_default()
                    };
                    if room_pub.is_empty() {
                        continue;
                    }
                    let name = inner
                        .d
                        .c
                        .as_ref()
                        .and_then(|content| content.get("name"))
                        .and_then(|value| value.as_str())
                        .map(str::to_owned);
                    let before = self.config.rooms.len();
                    self.add_room_if_missing(&room_pub, name, Some(envelope.clone()));
                    if self.config.rooms.len() > before {
                        rooms_added += 1;
                    }
                }
                "room.update" => {
                    let Some(room_pub) = inner.d.r.as_deref() else {
                        continue;
                    };
                    let Some(name) = inner
                        .d
                        .c
                        .as_ref()
                        .and_then(|content| content.get("name"))
                        .and_then(|value| value.as_str())
                    else {
                        continue;
                    };
                    if let Some(room) = self
                        .config
                        .rooms
                        .iter_mut()
                        .find(|room| room.pub_key == room_pub)
                    {
                        room.name = name.to_owned();
                    }
                }
                "node.meta" => {
                    let endpoints = inner
                        .d
                        .c
                        .as_ref()
                        .and_then(|content| content.get("endpoints"))
                        .and_then(|value| value.as_array())
                        .cloned()
                        .unwrap_or_default();
                    for endpoint in endpoints {
                        let Some(proto) = endpoint.get("proto").and_then(|value| value.as_str())
                        else {
                            continue;
                        };
                        if !matches!(proto, "ws" | "wss" | "http" | "https") {
                            continue;
                        }
                        let Some(host) = endpoint.get("host").and_then(|value| value.as_str())
                        else {
                            continue;
                        };
                        let port = endpoint
                            .get("port")
                            .and_then(|value| value.as_u64())
                            .unwrap_or(match proto {
                                "wss" | "https" => 443,
                                _ => 80,
                            });
                        let path = endpoint
                            .get("path")
                            .and_then(|value| value.as_str())
                            .filter(|path| *path != "/")
                            .unwrap_or("");
                        let url = format!("{proto}://{host}:{port}{path}");
                        if !self.config.relays.iter().any(|relay| relay == &url) {
                            self.config.relays.push(url);
                            relays_added += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        self.save_config()?;
        self.status = format!(
            "Imported {imported} envelope(s), {rooms_added} room(s), {relays_added} relay(s)"
        );
        Ok(())
    }

    fn export_invite(&mut self, room_id: &str) -> Result<()> {
        let room = self
            .config
            .rooms
            .iter()
            .find(|room| room.pub_key == room_id)
            .ok_or_else(|| anyhow!("Unknown room"))?;
        let create = room
            .room_create
            .as_ref()
            .ok_or_else(|| anyhow!("No invite data is available for this room yet; sync first"))?;
        self.export_text = discovery::encode_uri(std::slice::from_ref(create))?;
        self.export_kind = "invite".to_owned();
        self.status = "Room invite exported".to_owned();
        Ok(())
    }

    fn export_messages(&mut self, room_id: &str) -> Result<()> {
        let envelopes = self.pool.export_room(room_id)?;
        if envelopes.is_empty() {
            bail!("No messages to export yet");
        }
        self.export_text = discovery::encode_file(&envelopes)?;
        self.export_kind = "messages".to_owned();
        self.status = format!("Exported {} envelope(s)", envelopes.len());
        Ok(())
    }

    fn delete_message(&mut self, room_id: &str, target: &str) -> Result<()> {
        if target.trim().is_empty() {
            bail!("No message selected");
        }
        let node = self.node()?.clone();
        let room = self
            .config
            .rooms
            .iter()
            .find(|room| room.pub_key == room_id)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown room"))?;
        let delete = make_room_delete(&node, &room.pub_key, 1, target.trim())?;
        let outgoing = if let Some(room_identity) = &room.room_identity {
            wrap_commit(delete, room_identity, now_ms())?
        } else {
            delete
        };
        self.pool.put(&outgoing)?;
        if self.config.relays.is_empty() {
            self.status = "Message deleted locally".to_owned();
            return Ok(());
        }
        match self.exchange(vec![outgoing], &room.pub_key) {
            Ok(_) => self.status = "Message deleted".to_owned(),
            Err(error) => self.status = format!("Message deleted locally: {error:#}"),
        }
        Ok(())
    }

    fn flag_message(&mut self, room_id: &str, target: &str, category: &str) -> Result<()> {
        if target.trim().is_empty() {
            bail!("No message selected");
        }
        let node = self.node()?.clone();
        let room = self
            .config
            .rooms
            .iter()
            .find(|room| room.pub_key == room_id)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown room"))?;
        let category = if category.trim().is_empty() {
            "spam"
        } else {
            category.trim()
        };
        let flag = make_room_flag(&node, &room.pub_key, target.trim(), category, None)?;
        let outgoing = if let Some(room_identity) = &room.room_identity {
            wrap_commit(flag, room_identity, now_ms())?
        } else {
            flag
        };
        self.pool.put(&outgoing)?;
        if self.config.relays.is_empty() {
            self.status = "Message flagged locally".to_owned();
            return Ok(());
        }
        match self.exchange(vec![outgoing], &room.pub_key) {
            Ok(_) => self.status = format!("Message flagged as {category}"),
            Err(error) => self.status = format!("Message flagged locally: {error:#}"),
        }
        Ok(())
    }

    fn attest_node(&mut self, target: &str, level: &str) -> Result<()> {
        if target.trim().is_empty() {
            bail!("No node selected");
        }
        let node = self.node()?.clone();
        let attestation = make_side_attestation(&node, target.trim(), level, None)?;
        self.pool.put_attestation(&attestation)?;
        if let Some(room) = self.active_room().map(|room| room.pub_key.clone()) {
            let _ = self.exchange(vec![attestation], &room);
        }
        self.status = format!("Attestation recorded: {level}");
        Ok(())
    }

    fn block_node(&mut self, node_pub: &str, kind: &str, reason: &str) -> Result<()> {
        if node_pub.trim().is_empty() {
            bail!("No node selected");
        }
        let kind = if kind == "spam" { "spam" } else { "block" };
        let reason = reason.trim();
        self.pool.block_node(
            node_pub.trim(),
            kind,
            if reason.is_empty() {
                None
            } else {
                Some(reason)
            },
            now_ms(),
        )?;
        self.status = if kind == "spam" {
            "Node marked as spam".to_owned()
        } else {
            "Node blocked".to_owned()
        };
        Ok(())
    }

    fn unblock_node(&mut self, node_pub: &str) -> Result<()> {
        if !self.pool.unblock_node(node_pub)? {
            bail!("Node is not blocked");
        }
        self.status = "Node unblocked".to_owned();
        Ok(())
    }

    fn set_rtl(&mut self, enabled: bool) -> Result<()> {
        self.config.rtl = enabled;
        self.save_config()?;
        self.status = if enabled {
            "Right-to-left layout enabled".to_owned()
        } else {
            "Left-to-right layout enabled".to_owned()
        };
        Ok(())
    }

    fn set_notifications(&mut self, enabled: bool) -> Result<()> {
        self.config.notifications = enabled;
        self.save_config()?;
        self.status = if enabled {
            "Notifications enabled".to_owned()
        } else {
            "Notifications disabled".to_owned()
        };
        Ok(())
    }

    fn start_host(&mut self, port: u16, proto: &str) -> Result<()> {
        if self.host.is_some() {
            bail!("The relay host is already running");
        }
        if port == 0 {
            bail!("Port must be between 1 and 65535");
        }
        let proto = match proto {
            "http" => "http",
            "ws" | "" => "ws",
            other => bail!("Unsupported relay protocol: {other}"),
        };
        let addr: SocketAddr = format!("0.0.0.0:{port}")
            .parse()
            .context("Invalid relay port")?;
        std::net::TcpListener::bind(addr)
            .with_context(|| format!("Port {port} is already in use"))?;
        let node = self.node()?.clone();
        let rooms: HashMap<String, LocalRoom> = self
            .config
            .rooms
            .iter()
            .filter_map(|room| {
                room.room_identity
                    .clone()
                    .map(|identity| (room.pub_key.clone(), identity))
            })
            .collect();
        let metrics = Arc::new(RelayMetrics::new());
        let relay_config = Arc::new(RelayConfig {
            node,
            endpoints: vec![EndpointConfig {
                addr,
                proto: proto.to_owned(),
                path: Some("/".to_owned()),
                tls: None,
            }],
            pool: Arc::clone(&self.pool),
            rooms,
            metrics: Some(Arc::clone(&metrics)),
            batch_processor: None,
        });
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        std::thread::Builder::new()
            .name("toloo-relay-host".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let Ok(runtime) = runtime else {
                    return;
                };
                let result: Result<()> = runtime.block_on(async move {
                    tokio::select! {
                        result = run_relay(relay_config) => result,
                        _ = stop_rx => Ok(()),
                    }
                });
                if let Err(error) = result {
                    eprintln!("relay host stopped: {error:#}");
                }
            })?;
        self.config.host = Some(HostConfig {
            port,
            proto: proto.to_owned(),
        });
        self.save_config()?;
        self.host = Some(HostHandle {
            stop: stop_tx,
            metrics,
            port,
            proto: proto.to_owned(),
        });
        crate::platform::host_started(&format!("{proto}://0.0.0.0:{port}"));
        self.status = format!("Relay host listening on {proto}://0.0.0.0:{port}");
        Ok(())
    }

    fn stop_host(&mut self) -> Result<()> {
        let Some(host) = self.host.take() else {
            bail!("The relay host is not running");
        };
        let _ = host.stop.send(());
        crate::platform::host_stopped();
        self.status = "Relay host stopped".to_owned();
        Ok(())
    }

    fn send_message(&mut self, body: &str) -> Result<()> {
        let body = body.trim();
        if body.is_empty() {
            return Ok(());
        }
        let node = self.node()?.clone();
        let room = self
            .active_room()
            .cloned()
            .ok_or_else(|| anyhow!("Select a room first"))?;
        let now = now_ms();
        if now <= self.last_outgoing_ts {
            std::thread::sleep(Duration::from_millis(self.last_outgoing_ts - now + 1));
        }
        let draft = make_room_message(&node, &room.pub_key, 1, body, vec![])?;
        self.last_outgoing_ts = innermost(&draft).d.ts;
        let outgoing = if let Some(room_identity) = &room.room_identity {
            let timestamp = now_ms().max(innermost(&draft).d.ts);
            wrap_commit(draft, room_identity, timestamp)?
        } else {
            draft
        };
        self.pool.put(&outgoing)?;

        let offers = room
            .room_create
            .clone()
            .map(|create| vec![create, outgoing.clone()])
            .unwrap_or_else(|| vec![outgoing]);
        match self.exchange(offers, &room.pub_key) {
            Ok(_) => {
                if room.room_identity.is_some() {
                    let committed = self.commit_pending(&room)?;
                    self.status = format!("Sent and committed {committed} pending event(s)");
                } else {
                    self.status = "Message sent".to_owned();
                }
            }
            Err(error) => self.status = format!("Message saved locally: {error:#}"),
        }
        Ok(())
    }

    fn sync_active(&mut self) -> Result<()> {
        let room_id = self
            .active_room()
            .map(|room| room.pub_key.clone())
            .ok_or_else(|| anyhow!("Select a room first"))?;
        self.sync_room(&room_id)
    }

    fn sync_room(&mut self, room_id: &str) -> Result<()> {
        let node = self.node()?.clone();
        let room = self
            .config
            .rooms
            .iter()
            .find(|room| room.pub_key == room_id)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown room"))?;
        if self.config.relays.is_empty() {
            self.status = "No relay configured".to_owned();
            return Ok(());
        }

        let mut synced = 0usize;
        let mut reached = 0usize;
        let mut errors = Vec::new();
        for relay in self.config.relays.clone() {
            let config = PoolExchangeConfig {
                offer: Vec::new(),
                want_rooms: vec![WantRoom {
                    room_pub: room.pub_key.clone(),
                    after: 0,
                    limit: 500,
                }],
                timeout: Some(Duration::from_secs(30)),
            };
            match self
                .runtime
                .block_on(pool_exchange(&relay, &node, config, TlsVerify::AcceptAny))
            {
                Ok(envelopes) => {
                    reached += 1;
                    synced += envelopes.len();
                    for envelope in envelopes {
                        let _ = self.pool.put(&envelope);
                    }
                }
                Err(error) => errors.push(format!("{relay}: {error}")),
            }
        }

        let mut committed = 0;
        if room.room_identity.is_some() {
            committed = self.commit_pending(&room)?;
            self.pool.build_membership(&room.pub_key).ok();
        }
        if reached == 0 {
            bail!("{}", errors.join("; "));
        }
        self.status = if errors.is_empty() {
            format!("Synced {synced} envelope(s); committed {committed}")
        } else {
            format!(
                "Synced {synced} envelope(s); {reached} relay(s) reached; {} failed",
                errors.len()
            )
        };
        Ok(())
    }

    fn exchange(&mut self, offers: Vec<Envelope>, room_pub: &str) -> Result<usize> {
        let node = self.node()?.clone();
        if self.config.relays.is_empty() {
            bail!("No relay configured");
        }
        let mut total = 0usize;
        let mut reached = 0usize;
        let mut errors = Vec::new();
        for relay in self.config.relays.clone() {
            let config = PoolExchangeConfig {
                offer: offers.clone(),
                want_rooms: vec![WantRoom {
                    room_pub: room_pub.to_owned(),
                    after: 0,
                    limit: 500,
                }],
                timeout: Some(Duration::from_secs(30)),
            };
            match self
                .runtime
                .block_on(pool_exchange(&relay, &node, config, TlsVerify::AcceptAny))
            {
                Ok(envelopes) => {
                    reached += 1;
                    total += envelopes.len();
                    for envelope in envelopes {
                        let _ = self.pool.put(&envelope);
                    }
                }
                Err(error) => errors.push(format!("{relay}: {error}")),
            }
        }
        if reached == 0 {
            bail!("{}", errors.join("; "));
        }
        Ok(total)
    }

    fn committed_eids(&self, room_pub: &str) -> HashSet<String> {
        let mut ids = HashSet::new();
        for channel in [-2, -1, 0, 1] {
            if let Ok(entries) = self.pool.get_room_channel(room_pub, channel) {
                for entry in entries {
                    if depth(&entry.env) >= 2 {
                        ids.insert(eid(&entry.env));
                    }
                }
            }
        }
        ids
    }

    fn commit_pending(&mut self, room: &RoomConfig) -> Result<usize> {
        let Some(room_key) = room.room_identity.clone() else {
            return Ok(0);
        };
        let committed = self.committed_eids(&room.pub_key);
        let pending = self.pool.get_pending(
            &room.pub_key,
            &["room.message", "room.join", "room.leave", "room.invite"],
            100,
        )?;
        let mut seen = HashSet::new();
        let mut offers = Vec::new();
        let mut timestamp = now_ms();
        for entry in pending {
            let event_id = eid(&entry.env);
            if committed.contains(&event_id) || !seen.insert(event_id) {
                continue;
            }
            let inner_ts = innermost(&entry.env).d.ts;
            timestamp = timestamp.max(inner_ts).max(timestamp.saturating_add(1));
            let commit = wrap_commit((*entry.env).clone(), &room_key, timestamp)?;
            let _ = self.pool.put(&commit);
            offers.push(commit);
        }
        if offers.is_empty() {
            return Ok(0);
        }
        let count = offers.len();
        self.exchange(offers, &room.pub_key)?;
        Ok(count)
    }

    fn snapshot(&self) -> Snapshot {
        let node_pub = self
            .config
            .identity
            .as_ref()
            .map(|node| node.sig.pub_key.clone());
        let active = self.active_room();
        let blocked: HashSet<String> = self
            .pool
            .get_blocklist()
            .map(|entries| entries.into_iter().map(|entry| entry.0).collect())
            .unwrap_or_default();

        let rooms = self
            .config
            .rooms
            .iter()
            .filter(|room| {
                self.search.is_empty() || room.name.to_lowercase().contains(&self.search)
            })
            .map(|room| {
                let is_active = active
                    .map(|active| active.pub_key == room.pub_key)
                    .unwrap_or(false);
                let mark = self
                    .config
                    .read_marks
                    .get(&room.pub_key)
                    .copied()
                    .unwrap_or(0);
                let mut preview = String::new();
                let mut time_label = String::new();
                let mut latest_ts = 0u64;
                let mut unread_ids = HashSet::new();
                if let Ok(entries) = self.pool.get_room_channel(&room.pub_key, 1) {
                    for entry in entries {
                        let inner = innermost(&entry.env);
                        if inner.d.t != "room.message" {
                            continue;
                        }
                        let body = message_body(inner);
                        let from_self = node_pub.as_deref() == Some(inner.d.n.as_str());
                        if !from_self && !blocked.contains(&inner.d.n) && inner.d.ts > mark {
                            unread_ids.insert(message_key(&inner.d.n, inner.d.ts, &body));
                        }
                        if inner.d.ts > latest_ts {
                            latest_ts = inner.d.ts;
                            preview = preview_text(&body);
                            time_label = clock(inner.d.ts);
                        }
                    }
                }
                RoomView {
                    room_id: room.pub_key.clone(),
                    display_name: room.name.clone(),
                    initial: initial(&room.name),
                    selected: is_active,
                    has_key: room.room_identity.is_some(),
                    preview,
                    time_label,
                    unread: if is_active {
                        0
                    } else {
                        unread_ids.len() as i32
                    },
                    member_count: 0,
                }
            })
            .collect();

        let mut messages: Vec<MessageView> = Vec::new();
        let mut members_label = String::new();
        let mut active_room = String::new();
        let mut active_room_id = String::new();
        let mut room_key = String::new();
        let mut room_initial = "?".to_owned();
        let mut room_has_key = false;
        if let Some(room) = active {
            active_room = room.name.clone();
            active_room_id = room.pub_key.clone();
            room_key = short_key(&room.pub_key);
            room_initial = initial(&room.name);
            room_has_key = room.room_identity.is_some();
            if let Ok(membership) = self.pool.build_membership(&room.pub_key) {
                members_label = format!(
                    "{} member{}",
                    membership.members.len(),
                    if membership.members.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
            }
            if let Ok(entries) = self.pool.get_room_channel(&room.pub_key, 1) {
                let deleted: HashSet<String> = entries
                    .iter()
                    .filter_map(|entry| {
                        let inner = innermost(&entry.env);
                        if inner.d.t != "room.delete" {
                            return None;
                        }
                        inner
                            .d
                            .c
                            .as_ref()
                            .and_then(|content| content.get("target"))
                            .and_then(|value| value.as_str())
                            .map(str::to_owned)
                    })
                    .collect();
                let mut best: HashMap<String, (u8, MessageView, u64)> = HashMap::new();
                for entry in entries {
                    let inner = innermost(&entry.env);
                    if inner.d.t != "room.message" || blocked.contains(&inner.d.n) {
                        continue;
                    }
                    let body = message_body(&inner);
                    let event_id = eid(&entry.env);
                    if deleted.contains(&event_id) {
                        continue;
                    }
                    let layer = depth(&entry.env);
                    let key = message_key(&inner.d.n, inner.d.ts, &body);
                    let view = MessageView {
                        eid: event_id,
                        author: short_key(&inner.d.n),
                        author_id: inner.d.n.clone(),
                        author_initial: initial(&inner.d.n),
                        body,
                        clock: clock(inner.d.ts),
                        mine: node_pub.as_deref() == Some(inner.d.n.as_str()),
                        committed: layer >= 2,
                        status: if layer >= 2 {
                            "sent".to_owned()
                        } else {
                            "sending".to_owned()
                        },
                        day_label: day_label(inner.d.ts),
                        new_day: false,
                        grouped: false,
                    };
                    let replace = best
                        .get(&key)
                        .map(|(existing_depth, _, existing_ts)| {
                            (layer, inner.d.ts) > (*existing_depth, *existing_ts)
                        })
                        .unwrap_or(true);
                    if replace {
                        best.insert(key, (layer, view, inner.d.ts));
                    }
                }
                let mut ordered: Vec<(u64, MessageView)> =
                    best.into_values().map(|(_, view, ts)| (ts, view)).collect();
                ordered.sort_by_key(|(ts, _)| *ts);
                let skip = ordered.len().saturating_sub(MAX_VISIBLE_MESSAGES);
                let mut previous_day = String::new();
                let mut previous_author = String::new();
                messages = ordered
                    .into_iter()
                    .skip(skip)
                    .map(|(_, mut view)| {
                        view.new_day = view.day_label != previous_day;
                        view.grouped = !view.new_day && previous_author == view.author_id;
                        previous_day = view.day_label.clone();
                        previous_author = view.author_id.clone();
                        view
                    })
                    .collect();
            }
        }

        let identity_initial = node_pub
            .as_deref()
            .map(initial)
            .unwrap_or_else(|| "?".to_owned());
        let network_label = match self.config.relays.len() {
            0 => "No relays".to_owned(),
            1 => "1 relay".to_owned(),
            count => format!("{count} relays"),
        };
        let (host_running, host_address, host_connections) = if let Some(host) = &self.host {
            (
                true,
                format!("{}://0.0.0.0:{}", host.proto, host.port),
                host.metrics
                    .active
                    .load(std::sync::atomic::Ordering::Relaxed) as i32,
            )
        } else {
            (
                false,
                self.config
                    .host
                    .as_ref()
                    .map(|host| format!("{}://0.0.0.0:{}", host.proto, host.port))
                    .unwrap_or_default(),
                0,
            )
        };
        let blocked_views = self
            .pool
            .get_blocklist()
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|(node_pub, kind, reason, _)| BlockedView {
                        short: short_key(&node_pub),
                        node_pub,
                        kind,
                        reason: reason.unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Snapshot {
            has_identity: self.config.identity.is_some(),
            identity_label: node_pub
                .as_deref()
                .map(|value| short_key(value))
                .unwrap_or_else(|| "No identity".to_owned()),
            identity_initial,
            identity_sig_pub: self
                .config
                .identity
                .as_ref()
                .map(|node| node.sig.pub_key.clone())
                .unwrap_or_default(),
            identity_enc_pub: self
                .config
                .identity
                .as_ref()
                .map(|node| node.enc.pub_key.clone())
                .unwrap_or_default(),
            status: if self.status == "Ready" {
                String::new()
            } else {
                self.status.clone()
            },
            network_label,
            active_room,
            active_room_id,
            room_key,
            room_initial,
            room_has_key,
            members_label,
            rtl: self.config.rtl,
            notifications_enabled: self.config.notifications,
            warming: false,
            export_text: self.export_text.clone(),
            export_kind: self.export_kind.clone(),
            relays: self
                .config
                .relays
                .iter()
                .map(|url| RelayView { url: url.clone() })
                .collect(),
            blocked: blocked_views,
            host_running,
            host_address,
            host_connections,
            host_port: self
                .host
                .as_ref()
                .map(|host| host.port.to_string())
                .or_else(|| self.config.host.as_ref().map(|host| host.port.to_string()))
                .unwrap_or_else(|| "17701".to_owned()),
            rooms,
            messages,
        }
    }
}

pub fn run(data_dir: PathBuf, commands: Receiver<Command>, updates: Sender<Snapshot>) {
    let mut engine = match Engine::load(data_dir, Some(&updates)) {
        Ok(engine) => engine,
        Err(error) => {
            let mut snapshot = Snapshot::default();
            snapshot.status = format!("Startup failed: {error:#}");
            let _ = updates.send(snapshot);
            return;
        }
    };
    let _ = updates.send(engine.snapshot());

    loop {
        match commands.recv_timeout(SYNC_INTERVAL) {
            Ok(command) => {
                if let Err(error) = engine.handle(command) {
                    engine.status = format!("Error: {error:#}");
                }
                let _ = updates.send(engine.snapshot());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if engine.config.identity.is_some()
                    && engine.active_room().is_some()
                    && !engine.config.relays.is_empty()
                {
                    if let Err(error) = engine.sync_active() {
                        engine.status = format!("Sync error: {error:#}");
                    }
                    let _ = updates.send(engine.snapshot());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn initial(value: &str) -> String {
    value
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned())
}

fn preview_text(value: &str) -> String {
    let mut result: String = value.chars().take(44).collect();
    if value.chars().count() > 44 {
        result.push('…');
    }
    result
}

fn message_body(inner: &Envelope) -> String {
    inner
        .d
        .c
        .as_ref()
        .and_then(|content| content.get("body"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned()
}

fn message_key(author: &str, timestamp: u64, body: &str) -> String {
    format!("{author}\u{1f}{timestamp}\u{1f}{body}")
}

fn short_key(value: &str) -> String {
    if value.len() <= 14 {
        value.to_owned()
    } else {
        format!("{}…{}", &value[..8], &value[value.len() - 4..])
    }
}

fn clock(timestamp: u64) -> String {
    let seconds = (timestamp / 1000) % 86_400;
    format!("{:02}:{:02} UTC", seconds / 3_600, (seconds % 3_600) / 60)
}

fn day_label(timestamp: u64) -> String {
    let days = (timestamp / 86_400_000) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_engine() -> (Engine, PathBuf) {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "toloo-native-test-{}-{}-{id}",
            std::process::id(),
            now_ms()
        ));
        let engine = Engine::load(dir.clone(), None).expect("engine load");
        (engine, dir)
    }

    #[test]
    fn creates_identity_room_and_committed_message_locally() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        assert!(engine.config.identity.is_some());

        engine
            .handle(Command::CreateRoom("Test room".to_owned()))
            .unwrap();
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.rooms.len(), 1);
        assert!(snapshot.rooms[0].has_key);
        assert_eq!(snapshot.active_room, "Test room");
        assert!(snapshot.identity_label.contains('…'));

        engine
            .handle(Command::SendMessage("hello native".to_owned()))
            .unwrap();
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.messages.len(), 1);
        assert_eq!(snapshot.messages[0].body, "hello native");
        assert!(snapshot.messages[0].mine);
        assert!(snapshot.messages[0].committed);
        assert!(dir.join("config.json").exists());

        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unread_counts_only_incoming_messages() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        engine
            .handle(Command::CreateRoom("Alpha".to_owned()))
            .unwrap();
        let room_a = engine.config.rooms[0].pub_key.clone();
        engine
            .handle(Command::CreateRoom("Beta".to_owned()))
            .unwrap();
        let room_b = engine.config.rooms[1].pub_key.clone();

        engine
            .handle(Command::SendMessage("my own message".to_owned()))
            .unwrap();
        engine.handle(Command::SelectRoom(room_a)).unwrap();
        let unread = |engine: &Engine| {
            engine
                .snapshot()
                .rooms
                .iter()
                .find(|room| room.room_id == room_b)
                .map(|room| room.unread)
                .unwrap_or(-1)
        };
        assert_eq!(unread(&engine), 0);

        let room_identity = engine.config.rooms[1]
            .room_identity
            .clone()
            .expect("room identity");
        let (sig_pub, sig_seed) = ed25519_generate();
        let (enc_pub, enc_priv) = x25519_generate();
        let peer = LocalNode {
            sig: Keypair {
                pub_key: base64url::encode(&sig_pub),
                priv_key: base64url::encode(&sig_seed),
            },
            enc: Keypair {
                pub_key: base64url::encode(&enc_pub),
                priv_key: base64url::encode(&enc_priv),
            },
        };
        let draft = make_room_message(&peer, &room_b, 1, "hello from a peer", vec![]).unwrap();
        let commit = wrap_commit(draft, &room_identity, now_ms() + 1000).unwrap();
        engine.pool.put(&commit).unwrap();
        assert_eq!(unread(&engine), 1);

        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restores_identity_from_seeds() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        let identity = engine.config.identity.clone().unwrap();

        let (mut restored, restored_dir) = temp_engine();
        restored
            .handle(Command::RestoreIdentity {
                sig_seed: identity.sig.priv_key.clone(),
                enc_priv: identity.enc.priv_key.clone(),
            })
            .unwrap();
        assert_eq!(
            restored.config.identity.as_ref().unwrap().sig.pub_key,
            identity.sig.pub_key
        );
        assert_eq!(
            restored.config.identity.as_ref().unwrap().enc.pub_key,
            identity.enc.pub_key
        );

        drop(engine);
        drop(restored);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(restored_dir);
    }

    #[test]
    fn syncs_via_configured_relay() {
        let Ok(relay) = std::env::var("TOLOO_TEST_RELAY") else {
            return;
        };
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        engine.handle(Command::AddRelay(relay)).unwrap();
        engine
            .handle(Command::CreateRoom("Relay room".to_owned()))
            .unwrap();
        assert!(
            !engine.status.starts_with("Room saved locally"),
            "publish failed: {}",
            engine.status
        );
        engine.handle(Command::Sync).unwrap();
        assert!(
            !engine.status.starts_with("Sync error"),
            "sync failed: {}",
            engine.status
        );
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_invalid_room_key() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        assert!(engine
            .handle(Command::JoinRoom("not-a-real-room".to_owned()))
            .is_err());
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn renames_and_leaves_rooms() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        engine
            .handle(Command::CreateRoom("Old name".to_owned()))
            .unwrap();
        let room_id = engine.snapshot().active_room_id;
        engine
            .handle(Command::RenameRoom {
                room_id: room_id.clone(),
                name: "New name".to_owned(),
            })
            .unwrap();
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.rooms[0].display_name, "New name");
        assert_eq!(snapshot.active_room, "New name");

        engine.handle(Command::LeaveRoom(room_id)).unwrap();
        let snapshot = engine.snapshot();
        assert!(snapshot.rooms.is_empty());
        assert!(snapshot.active_room_id.is_empty());

        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn exports_and_imports_discovery() {
        let (mut source, source_dir) = temp_engine();
        source.handle(Command::CreateIdentity).unwrap();
        source
            .handle(Command::CreateRoom("Imported room".to_owned()))
            .unwrap();
        source
            .handle(Command::SendMessage("imported hello".to_owned()))
            .unwrap();
        let room_id = source.snapshot().active_room_id;
        source
            .handle(Command::ExportInvite(room_id.clone()))
            .unwrap();
        let invite = source.export_text.clone();
        assert!(invite.starts_with("toloo://"));
        source.handle(Command::ExportMessages(room_id)).unwrap();
        assert!(!source.export_text.is_empty());

        let (mut target, target_dir) = temp_engine();
        target.handle(Command::ImportDiscovery(invite)).unwrap();
        let snapshot = target.snapshot();
        assert_eq!(snapshot.rooms.len(), 1);
        assert_eq!(snapshot.rooms[0].display_name, "Imported room");

        drop(source);
        drop(target);
        let _ = std::fs::remove_dir_all(source_dir);
        let _ = std::fs::remove_dir_all(target_dir);
    }

    #[test]
    fn deletes_messages_and_blocks_authors() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        engine
            .handle(Command::CreateRoom("Chat".to_owned()))
            .unwrap();
        engine
            .handle(Command::SendMessage("to be deleted".to_owned()))
            .unwrap();
        let snapshot = engine.snapshot();
        let room_id = snapshot.active_room_id.clone();
        let target = snapshot.messages[0].eid.clone();
        let author = snapshot.messages[0].author_id.clone();
        assert_eq!(snapshot.messages.len(), 1);

        engine
            .handle(Command::DeleteMessage { room_id, target })
            .unwrap();
        assert!(engine.snapshot().messages.is_empty());

        engine
            .handle(Command::SendMessage("visible".to_owned()))
            .unwrap();
        assert_eq!(engine.snapshot().messages.len(), 1);
        engine
            .handle(Command::BlockNode {
                node_pub: author,
                kind: "block".to_owned(),
                reason: "test".to_owned(),
            })
            .unwrap();
        let snapshot = engine.snapshot();
        assert!(snapshot.messages.is_empty());
        assert_eq!(snapshot.blocked.len(), 1);
        engine
            .handle(Command::UnblockNode(snapshot.blocked[0].node_pub.clone()))
            .unwrap();
        assert_eq!(engine.snapshot().messages.len(), 1);

        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn starts_and_stops_the_relay_host() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        engine
            .handle(Command::StartHost {
                port,
                proto: "ws".to_owned(),
            })
            .unwrap();
        let snapshot = engine.snapshot();
        assert!(snapshot.host_running);
        assert!(snapshot.host_address.contains(&port.to_string()));

        let mut connected = false;
        for _ in 0..40 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                connected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(connected, "relay host did not accept a connection");

        engine.handle(Command::StopHost).unwrap();
        assert!(!engine.snapshot().host_running);
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn encrypts_and_restores_identity() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        let expected = engine.config.identity.clone().unwrap();
        engine
            .handle(Command::ExportEncryptedIdentity("correct horse".to_owned()))
            .unwrap();
        let blob = engine.export_text.clone();
        assert!(!blob.is_empty());

        let (mut restored, restored_dir) = temp_engine();
        restored
            .handle(Command::LoadEncryptedIdentity {
                blob,
                passphrase: "correct horse".to_owned(),
            })
            .unwrap();
        assert_eq!(
            restored.config.identity.as_ref().unwrap().sig.pub_key,
            expected.sig.pub_key
        );

        drop(engine);
        drop(restored);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(restored_dir);
    }

    #[test]
    #[ignore = "manual UI fixture seeder; set TOLOO_SEED_DIR"]
    fn seed_ui_fixture() {
        let dir = std::env::var_os("TOLOO_SEED_DIR")
            .map(PathBuf::from)
            .expect("TOLOO_SEED_DIR");
        let mut engine = Engine::load(dir.clone(), None).unwrap();
        if engine.config.identity.is_none() {
            engine.create_identity().unwrap();
        }
        if engine.config.rooms.is_empty() {
            engine.create_room("Fireflies").unwrap();
            engine.create_room("Night Shift").unwrap();
        }
        let room_id = engine.config.rooms[0].pub_key.clone();
        let room_identity = engine.config.rooms[0]
            .room_identity
            .clone()
            .expect("fixture room is owned");
        let (sig_pub, sig_seed) = ed25519_generate();
        let (enc_pub, enc_priv) = x25519_generate();
        let peer = LocalNode {
            sig: Keypair {
                pub_key: base64url::encode(&sig_pub),
                priv_key: base64url::encode(&sig_seed),
            },
            enc: Keypair {
                pub_key: base64url::encode(&enc_pub),
                priv_key: base64url::encode(&enc_priv),
            },
        };
        let messages = [
            "The signal is clean tonight.",
            "Relay hops look stable from here.",
            "Meet at the usual channel after dark.",
        ];
        for body in messages {
            let draft = make_room_message(&peer, &room_id, 1, body, vec![]).unwrap();
            // Message ids are channel:timestamp:author, so keep timestamps distinct.
            std::thread::sleep(std::time::Duration::from_millis(2));
            let commit = wrap_commit(draft, &room_identity, now_ms()).unwrap();
            engine.pool.put(&commit).unwrap();
        }
        engine.select_room(&room_id).unwrap();
        engine.save_config().unwrap();
    }

    #[test]
    fn searches_rooms_by_name() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        engine
            .handle(Command::CreateRoom("Alpha".to_owned()))
            .unwrap();
        engine
            .handle(Command::CreateRoom("Beta".to_owned()))
            .unwrap();
        engine.handle(Command::SetSearch("bet".to_owned())).unwrap();
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.rooms.len(), 1);
        assert_eq!(snapshot.rooms[0].display_name, "Beta");
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }
}
