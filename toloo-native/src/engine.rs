use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use toloo_lib::base64url;
use toloo_lib::crypto::{ed25519_generate, ed25519_pub_from_seed, x25519_generate, x25519_pub_from_seed};
use toloo_lib::discovery;
use toloo_lib::envelope::{depth, innermost, wrap_commit};
use toloo_lib::events::{make_room_create, make_room_join, make_room_message};
use toloo_lib::ids::eid;
use toloo_lib::pool::{Pool, PoolConfig};
use toloo_lib::transport::client::{pool_exchange, PoolExchangeConfig, TlsVerify, WantRoom};
use toloo_lib::types::{Envelope, Keypair, LocalNode, LocalRoom};

const SYNC_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum Command {
    CreateIdentity,
    RestoreIdentity { sig_seed: String, enc_priv: String },
    AddRelay(String),
    CreateRoom(String),
    JoinRoom(String),
    SelectRoom(String),
    Sync,
    SendMessage(String),
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub has_identity: bool,
    pub identity_label: String,
    pub status: String,
    pub active_room: String,
    pub members_label: String,
    pub rooms: Vec<RoomView>,
    pub messages: Vec<MessageView>,
}

#[derive(Debug, Clone)]
pub struct RoomView {
    pub room_id: String,
    pub display_name: String,
    pub selected: bool,
    pub has_key: bool,
}

#[derive(Debug, Clone)]
pub struct MessageView {
    pub author: String,
    pub body: String,
    pub clock: String,
    pub mine: bool,
    pub committed: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            has_identity: false,
            identity_label: "No identity".to_owned(),
            status: String::new(),
            active_room: String::new(),
            members_label: String::new(),
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoomConfig {
    pub_key: String,
    name: String,
    room_identity: Option<LocalRoom>,
    room_create: Option<Envelope>,
}

struct Engine {
    data_dir: PathBuf,
    config: Config,
    pool: Pool,
    runtime: tokio::runtime::Runtime,
    status: String,
}

impl Engine {
    fn load(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("creating {}", data_dir.display()))?;
        let config_path = data_dir.join("config.json");
        let config = if config_path.exists() {
            serde_json::from_slice(&std::fs::read(&config_path)?)
                .with_context(|| format!("parsing {}", config_path.display()))?
        } else {
            Config::default()
        };
        let pool_base = data_dir.join("pool");
        let pool = Pool::open(
            pool_base
                .to_str()
                .context("Toloo storage path is not valid UTF-8")?,
            PoolConfig::default(),
        )?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut engine = Self {
            data_dir,
            config,
            pool,
            runtime,
            status: "Ready".to_owned(),
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
            Command::AddRelay(url) => self.add_relay(&url),
            Command::CreateRoom(name) => self.create_room(&name),
            Command::JoinRoom(input) => self.join_room(&input),
            Command::SelectRoom(room_id) => self.select_room(&room_id),
            Command::Sync => self.sync_active(),
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
        self.config.identity = Some(LocalNode {
            sig: Keypair {
                pub_key: base64url::encode(&sig_pub),
                priv_key: sig_seed.trim().to_owned(),
            },
            enc: Keypair {
                pub_key: base64url::encode(&enc_pub),
                priv_key: enc_priv.trim().to_owned(),
            },
        });
        self.save_config()?;
        self.status = "Identity restored".to_owned();
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
            Some(if name.trim().is_empty() { "Untitled room" } else { name.trim() }),
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
        if self
            .config
            .rooms
            .iter()
            .any(|room| room.pub_key == room_pub)
        {
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
        if !self
            .config
            .rooms
            .iter()
            .any(|room| room.pub_key == room_id)
        {
            bail!("Unknown room");
        }
        self.config.active = Some(room_id.to_owned());
        self.save_config()?;
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
        let draft = make_room_message(&node, &room.pub_key, 1, body, vec![])?;
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
        let node = self.node()?.clone();
        let room = self
            .active_room()
            .cloned()
            .ok_or_else(|| anyhow!("Select a room first"))?;
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
        let node_pub = self.config.identity.as_ref().map(|node| node.sig.pub_key.clone());
        let active = self.active_room();
        let rooms = self
            .config
            .rooms
            .iter()
            .map(|room| RoomView {
                room_id: room.pub_key.clone(),
                display_name: room.name.clone(),
                selected: active.map(|active| active.pub_key == room.pub_key).unwrap_or(false),
                has_key: room.room_identity.is_some(),
            })
            .collect();

        let mut messages: Vec<MessageView> = Vec::new();
        let mut members_label = String::new();
        let mut active_room = String::new();
        if let Some(room) = active {
            active_room = room.name.clone();
            if let Ok(membership) = self.pool.build_membership(&room.pub_key) {
                members_label = format!(
                    "{} member{}",
                    membership.members.len(),
                    if membership.members.len() == 1 { "" } else { "s" }
                );
            }
            if let Ok(entries) = self.pool.get_room_channel(&room.pub_key, 1) {
                let mut best: HashMap<String, (u8, MessageView, u64)> = HashMap::new();
                for entry in entries {
                    let inner = innermost(&entry.env);
                    if inner.d.t != "room.message" {
                        continue;
                    }
                    let body = inner
                        .d
                        .c
                        .as_ref()
                        .and_then(|content| content.get("body"))
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let layer = depth(&entry.env);
                    let event_id = eid(&entry.env);
                    let view = MessageView {
                        author: short_key(&inner.d.n),
                        body,
                        clock: clock(inner.d.ts),
                        mine: node_pub.as_deref() == Some(inner.d.n.as_str()),
                        committed: layer >= 2,
                    };
                    let replace = best
                        .get(&event_id)
                        .map(|(existing_depth, _, _)| layer > *existing_depth)
                        .unwrap_or(true);
                    if replace {
                        best.insert(event_id, (layer, view, inner.d.ts));
                    }
                }
                let mut ordered: Vec<(u64, MessageView)> = best
                    .into_values()
                    .map(|(_, view, ts)| (ts, view))
                    .collect();
                ordered.sort_by_key(|(ts, _)| *ts);
                messages = ordered.into_iter().map(|(_, view)| view).collect();
            }
        }

        Snapshot {
            has_identity: self.config.identity.is_some(),
            identity_label: node_pub
                .as_deref()
                .map(|value| short_key(value))
                .unwrap_or_else(|| "No identity".to_owned()),
            status: self.status.clone(),
            active_room,
            members_label,
            rooms,
            messages,
        }
    }
}

pub fn run(data_dir: PathBuf, commands: Receiver<Command>, updates: Sender<Snapshot>) {
    let mut engine = match Engine::load(data_dir) {
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

fn short_key(value: &str) -> String {
    if value.len() <= 14 {
        value.to_owned()
    } else {
        format!("{}…{}", &value[..8], &value[value.len() - 4..])
    }
}

fn clock(timestamp: u64) -> String {
    let seconds = (timestamp / 1000) % 86_400;
    format!(
        "{:02}:{:02} UTC",
        seconds / 3_600,
        (seconds % 3_600) / 60
    )
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
        let engine = Engine::load(dir.clone()).expect("engine load");
        (engine, dir)
    }

    #[test]
    fn creates_identity_room_and_committed_message_locally() {
        let (mut engine, dir) = temp_engine();
        engine.handle(Command::CreateIdentity).unwrap();
        assert!(engine.config.identity.is_some());

        engine.handle(Command::CreateRoom("Test room".to_owned())).unwrap();
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
        engine.handle(Command::CreateRoom("Relay room".to_owned())).unwrap();
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
}
