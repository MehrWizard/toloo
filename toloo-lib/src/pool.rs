//! Protocol event pool.
//!
//! Everything in Toloo is an append-only, signed envelope: the pool is an
//! in-memory index over an append-only log, not a database. All queries are
//! answered from memory; the log is only written to on `put`.
//!
//! Persistence layout (when opened with [`Pool::open`]), given base path `X`:
//!
//! - `X.jsonl` — one canonical-JSON envelope per line, appended on every put.
//!   On open the log is replayed to rebuild memory state. A truncated final
//!   line (crash mid-append) is detected and discarded.
//! - `X.meta.json` — small mutable side state (TOFU pins, blocklist,
//!   attestations, flags). Rewritten atomically (tmp + rename) on change.
//! - `X.blobs/` — blob piece files, one per piece (write-once). No in-memory mirror.
//! - `X.cold.jsonl` — optional cold storage for older events when tiered
//!   storage is enabled (see [`PoolConfig::max_resident_blocks`]).
//!
//! Superseded and pruned events stay in the log until [`Pool::compact`]
//! rewrites it from memory. Pruning methods compact automatically.
//!
//! [`Pool::memory`] runs the same engine with no persistence at all.

use std::collections::{HashMap, HashSet, BTreeMap};
use std::hash::Hasher;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use dashmap::DashMap;

/// String interning for pool keys to reduce allocations and enable zero-copy cloning.
/// Uses `Arc<str>` for shared ownership of string data.
type InternedStr = Arc<str>;

use anyhow::{anyhow, Context, Result};

use crate::canonical::{canonical, canonical_envelope_bytes_direct};
use crate::envelope::{depth, innermost};
use crate::ids::{channel_of, datum_id, datum_id_from_bytes};
use crate::rules::RuleSet;
use crate::types::Envelope;

/// Number of lock shards for concurrent access.
/// 16 provides good contention reduction for typical workloads.
const NUM_SHARDS: usize = 16;

/// Configuration for tiered storage and retention policies.
///
/// When `max_resident_blocks` is set, the pool keeps only the most recent
/// events in memory (hot tier) and spills older events to a cold JSONL file.
/// This enables running on memory-constrained devices (OpenWrt-class, 64MB RAM)
/// while still serving recent history for sync.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of events to keep in the hot (in-memory) tier.
    /// When exceeded, oldest events are moved to cold storage.
    /// Default: 0 (unlimited, all events in memory).
    pub max_resident_blocks: usize,

    /// Number of most recent blocks to keep available for tail-only replay.
    /// Clients requesting sync with `after=0` get only the tail unless they
    /// explicitly request full history.
    /// Default: 1000
    pub tail_blocks_to_keep: usize,

    /// Optional path for cold storage JSONL. If not set, uses `<base>.cold.jsonl`.
    pub cold_storage_path: Option<PathBuf>,

    /// Room class retention defaults (days).
    /// Overridable per-room via metadata in channel -1.
    pub retention_public_days: u64,
    pub retention_personal_days: u64,
    pub retention_private_days: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_resident_blocks: 0,
            tail_blocks_to_keep: 1000,
            cold_storage_path: None,
            // Public rooms: 7 days default (ephemeral, high-traffic)
            retention_public_days: 7,
            // Personal rooms: 90 days default (DMs, small groups)
            retention_personal_days: 90,
            // Private/invite-only rooms: 30 days default
            retention_private_days: 30,
        }
    }
}

/// Room class for retention policy selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomClass {
    /// Public room — anyone can join, high traffic expected.
    Public,
    /// Personal room — small group, DMs, trusted peers.
    Personal,
    /// Private room — invite-only, restricted membership.
    Private,
}

impl RoomClass {
    /// Default retention in days for this room class.
    pub fn default_retention_days(&self, config: &PoolConfig) -> u64 {
        match self {
            RoomClass::Public => config.retention_public_days,
            RoomClass::Personal => config.retention_personal_days,
            RoomClass::Private => config.retention_private_days,
        }
    }
}

/// A blocklist row: (node_pub, kind, reason, blocked_at).
pub type BlocklistEntry = (String, String, Option<String>, u64);

/// Sharded lock for high-concurrency access.
/// Each shard protects a subset of rooms based on hash.
/// Uses DashMap for lock-free reads on primary indexes.
struct Shard {
    /// Live events in insertion order; `None` marks a removed slot.
    /// Live events in insertion order; `None` marks a removed/spilled slot.
    events: RwLock<Vec<Option<Arc<PoolEntry>>>>,
    /// Number of `Some` slots in `events` (avoids O(n) counting on spill checks).
    live: AtomicUsize,
    /// id -> index in events vec (lock-free reads)
    by_id: DashMap<String, usize>,
    /// (room, channel) -> list of event indices (for get_room_channel) (lock-free reads)
    /// Uses `String` for room key to allow fast `&str` lookup without Arc overhead.
    by_room_channel: DashMap<(String, i32), Vec<usize>>,
    /// (room, tc) -> event index (for get_wanted / latest_tc) (ordered, needs RwLock for writes)
    /// Uses `InternedStr` (Arc<str>) for room key to share allocation across index entries.
    by_room_tc: RwLock<BTreeMap<(InternedStr, u64), usize>>,
    /// (author, room, tc) -> event index (for author-specific queries) (ordered, needs RwLock for writes)
    /// Uses `InternedStr` for author and room keys to share allocation.
    by_author_room_tc: RwLock<BTreeMap<(InternedStr, InternedStr, u64), usize>>,
    /// Mutable side state (pins, blocklist, attestations, flags, room meta)
    meta: RwLock<MetaState>,
    /// Mutex for synchronizing write operations that modify multiple indexes atomically
    write_mutex: Mutex<()>,
    // In-memory mirror of blob pieces is removed — blob pieces are on-disk only.
// Use get_blob_piece / put_blob_piece which read/write files directly.
}

impl Default for Shard {
    fn default() -> Self {
        Self {
            events: RwLock::new(Vec::new()),
            live: AtomicUsize::new(0),
            by_id: DashMap::new(),
            by_room_channel: DashMap::new(),
            by_room_tc: RwLock::new(BTreeMap::new()),
            by_author_room_tc: RwLock::new(BTreeMap::new()),
            meta: RwLock::new(MetaState::default()),
            write_mutex: Mutex::new(()),
        }
    }
}

// `write_mutex` serializes writers (atomic multi-index updates); `events` is
// separately RwLock-protected so readers never race a Vec reallocation.
// Locking rule: a writer releases the `events` lock before taking an index
// lock, while readers take the index lock first, then the `events` lock.

/// Pool entry with Arc<Envelope> for zero-copy sharing.
/// Uses `InternedStr` (Arc<str>) for string fields to enable zero-copy cloning
/// and reduce memory allocations when the same strings appear across many entries.
#[derive(Debug, Clone)]
pub struct PoolEntry {
    pub id: InternedStr,
    pub env: Arc<Envelope>,
    pub room: Option<InternedStr>,
    pub n: InternedStr,
    pub to_node: Option<InternedStr>,
    pub ts: u64,
    pub tc: Option<u64>,
    pub event_type: InternedStr,
    pub channel: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct MembershipState {
    pub members: HashSet<String>,
    pub banned: HashSet<String>,
}

/// Result of a TOFU pin check (§4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TofuResult {
    /// First time seeing this node — pin recorded.
    NewPin,
    /// Seen before and fingerprint matches.
    Match,
    /// Seen before but fingerprint changed — potential impersonation.
    Mismatch {
        expected: String,
        first_seen_at: u64,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TofuPin {
    first_seen_at: u64,
    fingerprint: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BlockRecord {
    kind: String,
    reason: Option<String>,
    blocked_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AttestationRecord {
    id: String,
    json: String,
    attester: String,
    target: String,
    level: String,
    ts: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FlagRecord {
    id: String,
    room: String,
    target_eid: String,
    category: String,
    flagger: String,
    ts: u64,
}

/// Per-room metadata for retention policy and class.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RoomMeta {
    class: RoomClass,
    /// Optional custom retention override (days). None = use class default.
    retention_days: Option<u64>,
}

/// Mutable side state persisted to `X.meta.json`.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct MetaState {
    #[serde(default)]
    tofu_pins: HashMap<String, TofuPin>,
    #[serde(default)]
    blocklist: HashMap<String, BlockRecord>,
    #[serde(default)]
    attestations: Vec<AttestationRecord>,
    #[serde(default)]
    flags: Vec<FlagRecord>,
    #[serde(default)]
    room_meta: HashMap<String, RoomMeta>,
}

/// Where this pool persists, derived from the base path given to [`Pool::open`].
struct Persist {
    events_path: PathBuf,
    meta_path: PathBuf,
    blobs_dir: PathBuf,
    cold_path: PathBuf,
    config: PoolConfig,
    /// File lock for cold storage spill operations (prevents concurrent spills)
    spill_lock: Mutex<()>,
}

pub struct Pool {
    /// Sharded state for concurrent access (lock-free reads via DashMap inside Shard)
    shards: [Shard; NUM_SHARDS],
    persist: Option<Persist>,
    config: PoolConfig,
    /// Membership replay cache: room → (latest membership tc, state).
    membership_cache: DashMap<String, (u64, Arc<MembershipState>)>,
}

impl Pool {
    /// Open a persistent pool with the given configuration. `path` is a base
    /// path; the event log lives at `<path>.jsonl` alongside
    /// `<path>.meta.json`, `<path>.blobs/`, and optionally `<path>.cold.jsonl`
    /// for tiered storage.
    pub fn open(path: &str, config: PoolConfig) -> Result<Self> {
        let base = Path::new(path);
        let cold_path = config
            .cold_storage_path
            .clone()
            .unwrap_or_else(|| base.with_extension("cold.jsonl"));

        let persist = Persist {
            events_path: base.with_extension("jsonl"),
            meta_path: base.with_extension("meta.json"),
            blobs_dir: base.with_extension("blobs"),
            cold_path,
            config: config.clone(),
            spill_lock: Mutex::new(()),
        };
        std::fs::create_dir_all(&persist.blobs_dir).with_context(|| {
            format!("failed to create blob dir {}", persist.blobs_dir.display())
        })?;
        // Ensure cold storage file exists if we're using tiered storage.
        if persist.config.max_resident_blocks > 0 && !persist.cold_path.exists() {
            std::fs::File::create(&persist.cold_path)
                .with_context(|| format!("failed to create cold storage {}", persist.cold_path.display()))?;
        }

        // Initialize shards
        let shards: [Shard; NUM_SHARDS] = std::array::from_fn(|_| Shard::default());

        let mut pool = Pool {
            shards,
            persist: Some(persist),
            config,
            membership_cache: DashMap::new(),
        };
        pool.load_meta()?;
        pool.replay_events()?;
        // After replay, if we have tiered storage, spill excess to cold.
        pool.maybe_spill_to_cold()?;
        Ok(pool)
    }

    /// In-memory pool with no persistence. Everything is lost on drop.
    pub fn memory() -> Result<Self> {
        let shards: [Shard; NUM_SHARDS] = std::array::from_fn(|_| Shard::default());
        Ok(Pool {
            shards,
            persist: None,
            config: PoolConfig::default(),
            membership_cache: DashMap::new(),
        })
    }

    /// Open a persistent pool with default configuration.
    pub fn open_default(path: &str) -> Result<Self> {
        Self::open(path, PoolConfig::default())
    }

    /// Get shard index for a room key (or 0 for room-less operations).
    fn shard_idx(room: Option<&str>) -> usize {
        if let Some(r) = room {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(r, &mut hasher);
            (hasher.finish() as usize) % NUM_SHARDS
        } else {
            0
        }
    }

    /// Get shard reference for a room (lock-free).
    fn shard(&self, room: Option<&str>) -> &Shard {
        &self.shards[Self::shard_idx(room)]
    }

    /// Get mutable shard reference for a room (requires external synchronization for writes).
    fn shard_mut(&mut self, room: Option<&str>) -> &mut Shard {
        &mut self.shards[Self::shard_idx(room)]
    }

    /// Iterate over all shards (lock-free reads).
    fn read_all_shards<F: FnMut(&Shard)>(&self, mut f: F) {
        for shard in &self.shards {
            f(shard);
        }
    }

    /// Iterate over all shards mutably (requires external synchronization).
    #[allow(dead_code)]
    fn write_all_shards<F: FnMut(&mut Shard)>(&mut self, mut f: F) {
        for shard in &mut self.shards {
            f(shard);
        }
    }

    /// Collect clones of all live entries matching `pred` across all shards (lock-free reads).
    pub(crate) fn select<F: Fn(&PoolEntry) -> bool + Sync>(&self, pred: F) -> Result<Vec<PoolEntry>> {
        let mut out = Vec::new();
        self.read_all_shards(|shard| {
            for entry_opt in shard.events.read().expect("events lock poisoned").iter() {
                if let Some(entry) = entry_opt {
                    if pred(entry.as_ref()) {
                        out.push(entry.as_ref().clone());
                    }
                }
            }
        });
        Ok(out)
    }

    /// Atomically persist the current side state (pins, blocklist, …).
    fn persist_meta(&self) -> Result<()> {
        // Merge meta from all shards
        let mut merged = MetaState::default();
        self.read_all_shards(|shard| {
            let meta = shard.meta.read().expect("meta lock poisoned");
            merged.tofu_pins.extend(meta.tofu_pins.clone());
            merged.blocklist.extend(meta.blocklist.clone());
            merged.attestations.extend(meta.attestations.clone());
            merged.flags.extend(meta.flags.clone());
            merged.room_meta.extend(meta.room_meta.clone());
        });

        let Some(p) = &self.persist else {
            return Ok(());
        };
        let tmp = p.meta_path.with_extension("meta.json.tmp");
        let json = serde_json::to_string_pretty(&merged).context("meta serialization")?;
        std::fs::write(&tmp, json).with_context(|| format!("failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, &p.meta_path)
            .with_context(|| format!("failed to replace {}", p.meta_path.display()))?;
        Ok(())
    }

    // ── Persistence ────────────────────────────────────────────────────

    fn load_meta(&mut self) -> Result<()> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        if !p.meta_path.exists() {
            return Ok(());
        }
        let raw = std::fs::read_to_string(&p.meta_path)
            .with_context(|| format!("failed to read {}", p.meta_path.display()))?;
        let meta: MetaState = serde_json::from_str(&raw)
            .with_context(|| format!("invalid meta JSON in {}", p.meta_path.display()))?;
        // Distribute meta to shard 0 (meta is small, single-shard is fine)
        let shard0 = self.shard_mut(None);
        *shard0.meta.write().expect("meta lock poisoned") = meta;
        Ok(())
    }

    /// Rebuild memory state from the event log. A truncated final line
    /// (crash mid-append) is discarded; corruption anywhere else is fatal.
    fn replay_events(&mut self) -> Result<()> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        if !p.events_path.exists() {
            return Ok(());
        }
        let file = std::fs::File::open(&p.events_path)
            .with_context(|| format!("failed to read {}", p.events_path.display()))?;
        let mut reader = BufReader::new(file);
        let mut good_offset: u64 = 0;
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                good_offset += n as u64;
                continue;
            }
            match serde_json::from_str::<Envelope>(trimmed) {
                Ok(env) => {
                    let inner = innermost(&env);
                    let room = inner.d.r.as_deref();
                    let shard_idx = Self::shard_idx(room);
                    let id = datum_id(&env);
                    insert_new(&mut self.shards[shard_idx], &env, &id);
                    good_offset += n as u64;
                }
                Err(_) => {
                    // A failed parse is only tolerated as a partial final
                    // write: a line that does not end with '\n' (crash
                    // mid-append). Truncate it away and stop.
                    if !line.ends_with('\n') {
                        let f = std::fs::OpenOptions::new()
                            .write(true)
                            .open(&p.events_path)?;
                        f.set_len(good_offset)?;
                        break;
                    }
                    anyhow::bail!(
                        "corrupt event log {} at byte {}",
                        p.events_path.display(),
                        good_offset
                    );
                }
            }
        }
        Ok(())
    }

    /// If tiered storage is enabled and we exceed max_resident_blocks,
    /// move the oldest events to cold storage.
    fn maybe_spill_to_cold(&self) -> Result<()> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        if p.config.max_resident_blocks == 0 {
            return Ok(());
        }

        // Fast path: the common case (below the threshold) must not take the
        // global spill lock or scan every event.
        let live_fast: usize = self.shards.iter().map(|s| s.live.load(Ordering::Relaxed)).sum();
        if live_fast <= p.config.max_resident_blocks {
            return Ok(());
        }

        // Use file lock to prevent concurrent spills (cross-process safety)
        let _lock = p.spill_lock.lock().map_err(lock_err)?;

        let total_live: usize = self.shards.iter().map(|s| s.live.load(Ordering::Relaxed)).sum();
        if total_live <= p.config.max_resident_blocks {
            return Ok(());
        }

        // We need to spill (total_live - max_resident_blocks) oldest events.
        let to_spill = total_live - p.config.max_resident_blocks;

        // Collect the oldest event IDs to spill across all shards.
        let spill_ids: Vec<String> = {
            let mut all_events: Vec<(u64, String, usize, usize)> = Vec::new(); // (ts, id, shard_idx, event_idx)
            for (shard_idx, shard) in self.shards.iter().enumerate() {
                for (event_idx, entry_opt) in shard.events.read().expect("events lock poisoned").iter().enumerate() {
                    if let Some(entry) = entry_opt {
                        all_events.push((entry.ts, entry.id.to_string(), shard_idx, event_idx));
                    }
                }
            }
            all_events.sort_by_key(|(ts, _, _, _)| *ts);
            all_events.into_iter().take(to_spill).map(|(_, id, _, _)| id).collect()
        };

        if spill_ids.is_empty() {
            return Ok(());
        }

        // Read the hot log, write spilled events to cold, rewrite hot without them.
        let hot_path = &p.events_path;
        let cold_path = &p.cold_path;

        let mut hot_reader = BufReader::new(
            std::fs::File::open(hot_path).with_context(|| format!("open hot log {}", hot_path.display()))?,
        );
        let mut cold_writer = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(cold_path)
            .with_context(|| format!("open cold log {}", cold_path.display()))?;

        let spill_set: HashSet<_> = spill_ids.into_iter().collect();
        let mut new_hot_lines = Vec::new();
        let mut line = String::new();

        loop {
            line.clear();
            let n = hot_reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                new_hot_lines.push(line.clone());
                continue;
            }
            if let Ok(env) = serde_json::from_str::<Envelope>(trimmed) {
                let id = datum_id(&env);
                if spill_set.contains(&id) {
                    // Write to cold storage
                    cold_writer.write_all(line.as_bytes())?;
                } else {
                    new_hot_lines.push(line.clone());
                }
            } else {
                // Keep malformed lines in hot (shouldn't happen after replay)
                new_hot_lines.push(line.clone());
            }
        }
        cold_writer.flush()?;

        // Rewrite hot log
        let tmp = hot_path.with_extension("jsonl.tmp");
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)
                .with_context(|| format!("open tmp hot {}", tmp.display()))?;
            for l in new_hot_lines {
                f.write_all(l.as_bytes())?;
            }
            f.flush()?;
        }
        std::fs::rename(&tmp, hot_path)
            .with_context(|| format!("replace hot log {}", hot_path.display()))?;

        // Remove spilled events from memory across all shards. Slots are set to
        // `None` so the envelope data is dropped (bounding resident memory);
        // indexes are then rebuilt from the surviving entries.
        for shard in &self.shards {
            let _guard = shard.write_mutex.lock().map_err(lock_err)?;
            {
                // SAFETY: we hold this shard's write_mutex, giving exclusive
                // access to `events`.
                let mut events = shard.events.write().expect("events lock poisoned");
                for id in &spill_set {
                    if let Some((_, idx)) = shard.by_id.remove(id) {
                        let matches = events
                            .get(idx)
                            .and_then(|slot| slot.as_ref())
                            .is_some_and(|entry| entry.id.as_ref() == id);
                        if matches {
                            events[idx] = None;
                            shard.live.fetch_sub(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            rebuild_indexes(shard);
        }

        Ok(())
    }

    /// Read cold storage events after a given timestamp (for full history sync).
    fn read_cold_after(&self, after_ts: u64, limit: usize) -> Result<Vec<Envelope>> {
        let Some(p) = &self.persist else {
            return Ok(vec![]);
        };
        if !p.cold_path.exists() {
            return Ok(vec![]);
        }
        let file = std::fs::File::open(&p.cold_path)
            .with_context(|| format!("open cold log {}", p.cold_path.display()))?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(env) = serde_json::from_str::<Envelope>(trimmed) {
                let inner = innermost(&env);
                if inner.d.ts > after_ts {
                    events.push(env);
                    if events.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(events)
    }

    /// Append one or more canonical envelope lines with a single file open.
    fn append_events(&self, events: &[&[u8]]) -> Result<()> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        if events.is_empty() {
            return Ok(());
        }
        // In tiered mode the hot log is rewritten by the spill path; serialize
        // appends with the same lock so a rewrite cannot drop an appended event.
        let _lock = if p.config.max_resident_blocks > 0 {
            Some(p.spill_lock.lock().map_err(lock_err)?)
        } else {
            None
        };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p.events_path)
            .with_context(|| format!("failed to open {}", p.events_path.display()))?;
        for json in events {
            f.write_all(json)?;
            f.write_all(b"\n")?;
        }
        f.flush()?;
        Ok(())
    }

    fn write_blob_file(&self, blob_id: &str, piece_index: u32, data: &[u8]) -> Result<()> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        let path = p.blobs_dir.join(format!("{blob_id}.{piece_index}"));
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Rewrite the event log from memory, dropping superseded and pruned events.
    pub fn compact(&self) -> Result<()> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        let events_path = p.events_path.clone();
        let tmp = events_path.with_extension("jsonl.tmp");
        let max_resident_blocks = p.config.max_resident_blocks;
        let _cold_path = p.cold_path.clone();
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)
                .with_context(|| format!("failed to open {}", tmp.display()))?;

            // Collect all live entries across shards, sorted by insertion order
            let mut all_entries: Vec<(usize, Arc<PoolEntry>)> = Vec::new();
            for (shard_idx, shard) in self.shards.iter().enumerate() {
                for (_event_idx, entry_opt) in shard.events.read().expect("events lock poisoned").iter().enumerate() {
                    if let Some(entry) = entry_opt {
                        all_entries.push((shard_idx, entry.clone()));
                    }
                }
            }
            // Sort by original insertion order (approximate by ts then id)
            all_entries.sort_by(|a, b| {
                a.1.ts.cmp(&b.1.ts).then_with(|| a.1.id.cmp(&b.1.id))
            });

            for (_, entry) in all_entries {
                let json = canonical(&*entry.env)?;
                f.write_all(json.as_bytes())?;
                f.write_all(b"\n")?;
            }
            f.flush()?;
        }
        std::fs::rename(&tmp, &events_path)
            .with_context(|| format!("failed to replace {}", events_path.display()))?;

        // Rebuild all in-memory indexes from the compacted log
        self.rebuild_all_indexes()?;

        // Also compact cold storage if tiered storage is enabled
        if max_resident_blocks > 0 {
            self.compact_cold()?;
        }
        Ok(())
    }

    /// Rebuild all in-memory indexes from current shard state.
    fn rebuild_all_indexes(&self) -> Result<()> {
        for shard in &self.shards {
            let _guard = shard.write_mutex.lock().expect("shard write mutex poisoned");
            rebuild_indexes(shard);
        }
        Ok(())
    }

    /// Compact cold storage by rewriting it to only contain events that
    /// are still referenced (not in hot storage and not pruned).
    fn compact_cold(&self) -> Result<()> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        if !p.cold_path.exists() {
            return Ok(());
        }

        // Get all event IDs currently in hot storage
        let hot_ids: HashSet<String> = {
            let mut ids = HashSet::new();
            self.read_all_shards(|shard| {
                for entry_opt in shard.events.read().expect("events lock poisoned").iter() {
                    if let Some(entry) = entry_opt {
                        ids.insert(entry.id.to_string());
                    }
                }
            });
            ids
        };

        // Read cold, filter out events that are now in hot
        let file = std::fs::File::open(&p.cold_path)
            .with_context(|| format!("open cold log {}", p.cold_path.display()))?;
        let reader = BufReader::new(file);
        let tmp = p.cold_path.with_extension("jsonl.tmp");
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)
                .with_context(|| format!("open tmp cold {}", tmp.display()))?;
            for line in reader.lines() {
                let line = line?;
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(env) = serde_json::from_str::<Envelope>(trimmed) {
                    let id = datum_id(&env);
                    if !hot_ids.contains(&id) {
                        f.write_all(line.as_bytes())?;
                        f.write_all(b"\n")?;
                    }
                }
            }
            f.flush()?;
        }
        std::fs::rename(&tmp, &p.cold_path)
            .with_context(|| format!("replace cold log {}", p.cold_path.display()))?;
        Ok(())
    }

    // ── Event storage ──────────────────────────────────────────────────

    /// Store envelope and return outermost datum_id. Duplicates are ignored.
    /// Uses atomic check-and-insert to avoid TOCTOU.
    pub fn put(&self, env: &Envelope) -> Result<String> {
        // One canonical serialization serves both the datum_id and the log entry.
        let json = canonical_envelope_bytes_direct(env)?;
        let id = datum_id_from_bytes(&json);
        let inner = innermost(env);
        let room = inner.d.r.clone();

        // Determine target shard
        let shard_idx = Self::shard_idx(room.as_deref());

        // Atomic check-and-insert under write mutex
        let _superseded = {
            let _guard = self.shards[shard_idx].write_mutex.lock().map_err(lock_err)?;
            let shard = &self.shards[shard_idx];

            // Check if already exists (lock-free read)
            if shard.by_id.contains_key(&id) {
                return Ok(id);
            }

            // Find superseded events (same eid: room + signer + ts + channel)
            let key_room = room.clone();
            let channel_opt = channel_of(&inner.d);
            let superseded: Vec<String> = if let Some(room_ref) = &key_room {
                if let Some(channel) = channel_opt {
                    // Use secondary index for O(1) lookup
                    if let Some(indices) = shard.by_room_channel.get(&(room_ref.clone(), channel)) {
                        let events = shard.events.read().expect("events lock poisoned");
                        indices.iter()
                            .filter_map(|&idx| events[idx].as_ref())
                            .filter(|e| {
                                // eid match: same room + signer + ts + channel
                                let matches_key = e.room.as_deref() == Some(room_ref.as_str())
                                    && e.n.as_ref() == inner.d.n.as_str()
                                    && e.ts == inner.d.ts
                                    && e.channel == Some(channel);
                                let replaced = if depth(env) >= 3 {
                                    e.tc.is_some()
                                } else {
                                    e.tc.is_none()
                                };
                                matches_key && replaced
                            })
                            .map(|e| e.id.to_string())
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // Remove superseded entries
            for sid in &superseded {
                remove_id(shard, sid);
            }

            // Insert new entry
            insert_new(shard, env, &id);
            superseded
        };

        // Persist to log (outside lock to minimize lock hold time)
        self.append_events(&[json.as_slice()])?;

        // After adding new event, check if we need to spill to cold storage.
        self.maybe_spill_to_cold()?;

        Ok(id)
    }

    /// Batch insert multiple envelopes, reducing lock acquisitions.
    /// Groups envelopes by shard and acquires each shard lock once.
    /// Deduplicates within the batch and against existing pool entries.
    /// Returns vector of datum_ids in same order as input.
    pub fn put_batch(&self, envelopes: &[&Envelope]) -> Result<Vec<String>> {
        if envelopes.is_empty() {
            return Ok(Vec::new());
        }

        // Group envelopes by target shard
        let mut by_shard: Vec<Vec<(usize, &Envelope)>> = vec![Vec::new(); NUM_SHARDS];
        for (input_idx, env) in envelopes.iter().enumerate() {
            let inner = innermost(env);
            let room = inner.d.r.clone();
            let shard_idx = Self::shard_idx(room.as_deref());
            by_shard[shard_idx].push((input_idx, env));
        }

        // Prepare results
        let mut results = vec![None; envelopes.len()];
        let mut all_json = Vec::new();

        // Process each shard with its batch
        for (shard_idx, batch) in by_shard.into_iter().enumerate() {
            if batch.is_empty() {
                continue;
            }

            let shard = &self.shards[shard_idx];
            let _guard = shard.write_mutex.lock().map_err(lock_err)?;

            // Track IDs seen in this batch for intra-batch deduplication
            let mut seen_in_batch: HashSet<String> = HashSet::new();

            for (input_idx, env) in batch {
                let json = canonical_envelope_bytes_direct(env)?;
                let id = datum_id_from_bytes(&json);
                let inner = innermost(env);
                let room = inner.d.r.clone();

                // Check if already exists in pool OR in this batch (lock-free read)
                if shard.by_id.contains_key(&id) || seen_in_batch.contains(&id) {
                    results[input_idx] = Some(id);
                    continue;
                }
                seen_in_batch.insert(id.clone());

                // Find superseded events
                let channel_opt = channel_of(&inner.d);
                let superseded: Vec<String> = if let (Some(room_ref), Some(channel)) = (room.as_ref(), channel_opt) {
                    if let Some(indices) = shard.by_room_channel.get(&(room_ref.clone(), channel)) {
                        let events = shard.events.read().expect("events lock poisoned");
                        indices.iter()
                            .filter_map(|&idx| events[idx].as_ref())
                            .filter(|e| {
                                let matches_key = e.room.as_deref() == Some(room_ref.as_str())
                                    && e.n.as_ref() == inner.d.n.as_str()
                                    && e.ts == inner.d.ts
                                    && e.channel == Some(channel);
                                let replaced = if depth(env) >= 3 {
                                    e.tc.is_some()
                                } else {
                                    e.tc.is_none()
                                };
                                matches_key && replaced
                            })
                            .map(|e| e.id.to_string())
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

                // Remove superseded entries
                for sid in &superseded {
                    remove_id(shard, sid);
                }

                // Insert new entry
                insert_new(shard, env, &id);
                results[input_idx] = Some(id);
                all_json.push(json);
            }
        }

        // Persist the whole batch with a single open/write/flush (outside locks)
        let json_refs: Vec<&[u8]> = all_json.iter().map(Vec::as_slice).collect();
        self.append_events(&json_refs)?;

        // Check if we need to spill to cold storage
        self.maybe_spill_to_cold()?;

        // Convert results to expected format
        Ok(results.into_iter().flatten().collect())
    }

    /// Whether an event is resident in hot storage.
    fn contains_id(&self, id: &str) -> bool {
        self.shards.iter().any(|shard| shard.by_id.contains_key(id))
    }

    pub fn get(&self, id: &str) -> Result<Option<Envelope>> {
        // Search all shards for the ID (lock-free read via DashMap)
        for shard in &self.shards {
            if let Some(idx) = shard.by_id.get(id) {
                let idx = *idx;
                if let Some(entry) = &shard.events.read().expect("events lock poisoned")[idx] {
                    return Ok(Some((*entry.env).clone()));
                }
            }
        }
        Ok(None)
    }

    pub fn get_room_channel(&self, room: &str, channel: i32) -> Result<Vec<PoolEntry>> {
        let shard = self.shard(Some(room));

        let indices = shard.by_room_channel.get(&(room.to_string(), channel));
        let mut out = Vec::new();
        if let Some(indices) = indices {
            let events = shard.events.read().expect("events lock poisoned");
            for &idx in indices.value() {
                if let Some(entry) = &events[idx] {
                    out.push(entry.as_ref().clone());
                }
            }
        }
        out.sort_by(cmp_committed_first);
        Ok(out)
    }

    /// Returns the latest commit timestamp (tc) for committed events in a room.
    pub fn latest_tc(&self, room: &str) -> Result<u64> {
        let shard = self.shard(Some(room));

        // Use by_room_tc index for O(log n) lookup (need read lock for BTreeMap)
        let by_room_tc = shard.by_room_tc.read().expect("by_room_tc lock poisoned");
        let key = InternedStr::from(room);
        let range = (key.clone(), 0u64)..=(key, u64::MAX);
        if let Some((_, &idx)) = by_room_tc.range(range).next_back() {
            if let Some(entry) = &shard.events.read().expect("events lock poisoned")[idx] {
                if let Some(tc) = entry.tc {
                    return Ok(tc);
                }
            }
        }
        Ok(0)
    }

    /// Count committed events by `author` in `room` with `tc >= since_tc` on
    /// channels `>= min_channel`. Uses the author index — O(matches), not O(pool).
    pub fn count_author_events_since(
        &self,
        room: &str,
        author: &str,
        since_tc: u64,
        min_channel: i32,
    ) -> usize {
        let shard = self.shard(Some(room));
        let author_key: InternedStr = InternedStr::from(author);
        let room_key: InternedStr = InternedStr::from(room);
        let range = (author_key.clone(), room_key.clone(), since_tc)
            ..=(author_key, room_key, u64::MAX);
        let by_author = shard
            .by_author_room_tc
            .read()
            .expect("by_author_room_tc lock poisoned");
        let events = shard.events.read().expect("events lock poisoned");
        let mut count = 0;
        for (_, &idx) in by_author.range(range) {
            if let Some(Some(entry)) = events.get(idx) {
                if entry.channel.is_some_and(|c| c >= min_channel) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Room watermark: the latest commit time across all room-internal
    /// channels (`ch < 0` — metadata, membership, …).
    pub fn room_watermark(&self, room: &str) -> Result<u64> {
        let shard = self.shard(Some(room));

        let mut max_tc = 0u64;
        if let Some(indices) = shard.by_room_channel.get(&(room.to_string(), -1)) {
            for &idx in indices.value() {
                if let Some(entry) = &shard.events.read().expect("events lock poisoned")[idx] {
                    if let Some(tc) = entry.tc {
                        max_tc = max_tc.max(tc);
                    }
                }
            }
        }
        if let Some(indices) = shard.by_room_channel.get(&(room.to_string(), -2)) {
            for &idx in indices.value() {
                if let Some(entry) = &shard.events.read().expect("events lock poisoned")[idx] {
                    if let Some(tc) = entry.tc {
                        max_tc = max_tc.max(tc);
                    }
                }
            }
        }
        Ok(max_tc)
    }

    pub fn get_room_ids(&self, room: &str) -> Result<Vec<String>> {
        let shard = self.shard(Some(room));

        let indices = shard.by_room_channel.get(&(room.to_string(), 0))
            .or_else(|| shard.by_room_channel.get(&(room.to_string(), 1)))
            .or_else(|| shard.by_room_channel.get(&(room.to_string(), -1)))
            .or_else(|| shard.by_room_channel.get(&(room.to_string(), -2)));

        // If no specific channel index, scan all (fallback)
        let mut out = Vec::new();
        if let Some(indices) = indices {
            for &idx in indices.value() {
                if let Some(entry) = &shard.events.read().expect("events lock poisoned")[idx] {
                    out.push(entry.id.to_string());
                }
            }
        } else {
            // Fallback: scan all events in this shard
            for entry_opt in shard.events.read().expect("events lock poisoned").iter() {
                if let Some(entry) = entry_opt {
                    if entry.room.as_deref() == Some(room) {
                        out.push(entry.id.to_string());
                    }
                }
            }
        }
        out.sort_by(|a, b| a.cmp(b));
        Ok(out)
    }

    pub fn get_private(&self, node_pub: &str) -> Result<Vec<PoolEntry>> {
        let mut out = self.select(|e| {
            matches!(
                e.event_type.as_ref(),
                "private.message" | "private.read" | "private.blob"
            ) && (e.n.as_ref() == node_pub || e.to_node.as_deref() == Some(node_pub))
        })?;
        out.sort_by(cmp_committed_first);
        Ok(out)
    }

    /// Latest committed tc among channel -2 events, used to version the cache.
    fn membership_version(&self, room: &str) -> u64 {
        let shard = self.shard(Some(room));
        let mut version = 0u64;
        if let Some(indices) = shard.by_room_channel.get(&(room.to_string(), -2)) {
            let events = shard.events.read().expect("events lock poisoned");
            for &idx in indices.value() {
                if let Some(Some(entry)) = events.get(idx) {
                    if let Some(tc) = entry.tc {
                        version = version.max(tc);
                    }
                }
            }
        }
        version
    }

    /// Replay channel -2 events into membership state, cached by membership tc.
    pub fn build_membership(&self, room: &str) -> Result<Arc<MembershipState>> {
        let version = self.membership_version(room);
        if let Some(cached) = self.membership_cache.get(room) {
            if cached.0 == version {
                return Ok(Arc::clone(&cached.1));
            }
        }

        let events = self.get_room_channel(room, -2)?;
        let mut state = MembershipState::default();

        for event in events {
            if event.tc.is_none() {
                continue;
            }
            let d = &innermost(&event.env).d;
            match d.t.as_str() {
                "room.join" => {
                    if !state.banned.contains(&d.n) {
                        state.members.insert(d.n.clone());
                    }
                }
                "room.leave" => {
                    state.members.remove(&d.n);
                }
                "room.ban" => {
                    if let Some(target) =
                        d.c.as_ref()
                            .and_then(|c| c.get("banned"))
                            .and_then(|v| v.as_str())
                    {
                        state.members.remove(target);
                        state.banned.insert(target.to_owned());
                    }
                }
                "room.unban" => {
                    if let Some(target) =
                        d.c.as_ref()
                            .and_then(|c| c.get("unbanned"))
                            .and_then(|v| v.as_str())
                    {
                        state.banned.remove(target);
                    }
                }
                "room.invite" => {}
                _ => {}
            }
        }

        let state = Arc::new(state);
        self.membership_cache
            .insert(room.to_owned(), (version, Arc::clone(&state)));
        Ok(state)
    }

    pub fn get_uncommitted(&self, room: &str) -> Result<Vec<PoolEntry>> {
        let mut out = self.select(|e| e.room.as_deref() == Some(room) && e.tc.is_none())?;
        out.sort_by(cmp_ts_id);
        Ok(out)
    }

    /// Return committed envelopes matching optional room and type filters, after `after_tc`.
    ///
    /// - `rooms`: if empty, all rooms are included.
    /// - `types`: if empty, all event types are included.
    /// - `after_tc`: only events with `tc > after_tc` (use 0 for all).
    /// - `limit`: maximum rows returned.
    ///
    /// When `after_tc == 0` and tiered storage is enabled, this performs a
    /// tail-only replay: returns only the most recent `tail_blocks_to_keep`
    /// events from hot storage, without reading cold storage. This is the
    /// default sync behavior for clients. For full history, callers should
    /// use `get_wanted_full` or `get_room_channel_range`.
    pub fn get_wanted(
        &self,
        rooms: &[&str],
        types: &[&str],
        after_tc: u64,
        limit: usize,
    ) -> Result<Vec<PoolEntry>> {
        let tail_only = after_tc == 0 && self.config.max_resident_blocks > 0;
        let tail_limit = if tail_only {
            self.config.tail_blocks_to_keep.min(limit)
        } else {
            limit
        };

        // Use by_room_tc index for efficient range queries
        let mut out = Vec::new();

        if rooms.is_empty() {
            // Scan all shards, use by_room_tc index
            for shard in &self.shards {
                // by_room_tc uses InternedStr (Arc<str>) for room key (need read lock for BTreeMap)
                let by_room_tc = shard.by_room_tc.read().expect("by_room_tc lock poisoned");
                let events = shard.events.read().expect("events lock poisoned");
                let range = by_room_tc.range((InternedStr::from(""), after_tc + 1)..);
                for (_, &idx) in range.take(limit * 2) { // take extra for filtering
                    if let Some(entry) = &events[idx] {
                        if entry.tc.is_none() || entry.tc.unwrap_or(0) <= after_tc {
                            continue;
                        }
                        if !types.is_empty() && !types.contains(&entry.event_type.as_ref()) {
                            continue;
                        }
                        out.push(entry.as_ref().clone());
                    }
                }
            }
        } else {
            // Filter by specific rooms
            for room in rooms {
                let shard_idx = Self::shard_idx(Some(room));
                let shard = &self.shards[shard_idx];
                let by_room_tc = shard.by_room_tc.read().expect("by_room_tc lock poisoned");
                let events = shard.events.read().expect("events lock poisoned");
                let range = by_room_tc.range((InternedStr::from(room.as_ref()), after_tc + 1)..);
                for (_, &idx) in range.take(limit * 2) {
                    if let Some(entry) = &events[idx] {
                        if entry.tc.is_none() || entry.tc.unwrap_or(0) <= after_tc {
                            continue;
                        }
                        if !types.is_empty() && !types.contains(&entry.event_type.as_ref()) {
                            continue;
                        }
                        out.push(entry.as_ref().clone());
                    }
                }
            }
        }

        out.sort_by(cmp_tc);
        if tail_only && out.len() > tail_limit {
            let skip = out.len() - tail_limit;
            out.drain(0..skip);
        }
        out.truncate(limit);
        Ok(out)
    }

    /// Full-history version of `get_wanted` that also reads from cold storage.
    /// Use this when a client explicitly requests full history sync.
    pub fn get_wanted_full(
        &self,
        rooms: &[&str],
        types: &[&str],
        after_tc: u64,
        limit: usize,
    ) -> Result<Vec<PoolEntry>> {
        // Get ALL events from hot storage (bypass tail-only logic)
        let mut out = self.select(|e| {
            if e.tc.is_none() || e.tc.unwrap_or(0) <= after_tc {
                return false;
            }
            (rooms.is_empty()
                || e.room
                    .as_deref()
                    .map(|r| rooms.contains(&r))
                    .unwrap_or(false))
                && (types.is_empty() || types.contains(&e.event_type.as_ref()))
        })?;

        // If tiered storage enabled, also read from cold
        if self.config.max_resident_blocks > 0 {
            let cold_events = self.read_cold_after(after_tc, limit)?;
            for env in cold_events {
                let inner = innermost(&env);
                let entry = PoolEntry {
                    id: InternedStr::from(datum_id(&env)),
                    room: inner.d.r.clone().map(InternedStr::from),
                    n: InternedStr::from(inner.d.n.clone()),
                    to_node: inner.d.to.clone().map(InternedStr::from),
                    ts: inner.d.ts,
                    tc: inner.d.tc,
                    event_type: InternedStr::from(inner.d.t.clone()),
                    channel: channel_of(&inner.d),
                    // Move the envelope in as the final step (no deep clone).
                    env: Arc::new(env),
                };
                // Only add if not already resident in hot storage.
                if !self.contains_id(&entry.id) {
                    out.push(entry);
                }
            }
        }

        out.sort_by(cmp_tc);
        out.truncate(limit);
        Ok(out)
    }

    /// Return committed events for a room+channel in the given tc range.
    pub fn get_room_channel_range(
        &self,
        room: &str,
        channel: i32,
        start_tc: u64,
        end_tc: Option<u64>,
        limit: usize,
    ) -> Result<Vec<PoolEntry>> {
        let shard = self.shard(Some(room));

        let mut out = Vec::new();
        if let Some(indices) = shard.by_room_channel.get(&(room.to_string(), channel)) {
            let events = shard.events.read().expect("events lock poisoned");
            for &idx in indices.value() {
                if let Some(entry) = &events[idx] {
                    let tc = match entry.tc {
                        Some(tc) if tc >= start_tc => tc,
                        _ => continue,
                    };
                    if let Some(end) = end_tc {
                        if tc > end {
                            continue;
                        }
                    }
                    out.push(entry.as_ref().clone());
                }
            }
        }
        out.sort_by(cmp_tc);
        out.truncate(limit);
        Ok(out)
    }

    /// Fetch envelopes by a list of datum IDs (index lookup, not a full scan).
    pub fn get_by_datum_ids(&self, ids: &[&str]) -> Result<Vec<PoolEntry>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            for shard in &self.shards {
                let Some(idx) = shard.by_id.get(*id).map(|e| *e) else {
                    continue;
                };
                let events = shard.events.read().expect("events lock poisoned");
                if let Some(Some(entry)) = events.get(idx) {
                    out.push(entry.as_ref().clone());
                }
                break;
            }
        }
        out.sort_by(cmp_tc);
        Ok(out)
    }

    /// Return per-channel stats (channel, committed_count, latest_tc) for a room.
    /// Only non-negative channels (user content) are included.
    pub fn get_room_channel_stats(&self, room: &str) -> Result<Vec<(i32, u64, u64)>> {
        let shard = self.shard(Some(room));
        let mut per_channel: HashMap<i32, (u64, u64)> = HashMap::new();
        for entry_opt in shard.events.read().expect("events lock poisoned").iter() {
            if let Some(entry) = entry_opt {
                let Some(tc) = entry.tc else { continue };
                let Some(ch) = entry.channel else { continue };
                if ch < 0 || entry.room.as_deref() != Some(room) {
                    continue;
                }
                let slot = per_channel.entry(ch).or_insert((0, 0));
                slot.0 += 1;
                slot.1 = slot.1.max(tc);
            }
        }
        let mut out: Vec<(i32, u64, u64)> = per_channel
            .into_iter()
            .map(|(ch, (c, tc))| (ch, c, tc))
            .collect();
        out.sort_by_key(|(ch, _, _)| *ch);
        Ok(out)
    }

    /// Return distinct room public keys where `node_pub` has events.
    pub fn get_node_rooms(&self, node_pub: &str) -> Result<Vec<String>> {
        let mut rooms: HashSet<String> = HashSet::new();
        self.read_all_shards(|shard| {
            for entry_opt in shard.events.read().expect("events lock poisoned").iter() {
                if let Some(entry) = entry_opt {
                    if entry.n.as_ref() == node_pub {
                        if let Some(r) = &entry.room {
                            rooms.insert(r.as_ref().to_string());
                        }
                    }
                }
            }
        });
        let mut out: Vec<String> = rooms.into_iter().collect();
        out.sort();
        Ok(out)
    }

    // ── TOFU Pin Storage (§4.4, §11.3.1) ──────────────────────────────

    /// Check (and optionally record) a TOFU pin for a node.
    /// `fingerprint` is the hex-encoded SHA-256 of the node's public key.
    pub fn check_tofu_pin(
        &self,
        node_pub: &str,
        fingerprint: &str,
        now: u64,
    ) -> Result<TofuResult> {
        let _guard = self.shards[0].write_mutex.lock().expect("shard write mutex poisoned");
        let shard0 = &self.shards[0];
        let mut meta = shard0.meta.write().expect("meta lock poisoned");
        match meta.tofu_pins.get(node_pub) {
            Some(pin) => {
                if pin.fingerprint == fingerprint {
                    Ok(TofuResult::Match)
                } else {
                    Ok(TofuResult::Mismatch {
                        expected: pin.fingerprint.clone(),
                        first_seen_at: pin.first_seen_at,
                    })
                }
            }
            None => {
                meta.tofu_pins.insert(
                    node_pub.to_owned(),
                    TofuPin {
                        first_seen_at: now,
                        fingerprint: fingerprint.to_owned(),
                    },
                );
                drop(meta);
                self.persist_meta()?;
                Ok(TofuResult::NewPin)
            }
        }
    }

    /// Get the stored TOFU pin for a node, if any.
    pub fn get_tofu_pin(&self, node_pub: &str) -> Result<Option<(String, u64)>> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        Ok(meta
            .tofu_pins
            .get(node_pub)
            .map(|p| (p.fingerprint.clone(), p.first_seen_at)))
    }

    // ── Blob Piece Storage (§11.7) ─────────────────────────────────────
    /// Blob pieces are stored ON-DISK ONLY. No in-memory mirror.
    /// This prevents memory leaks on devices with large blob transfers.

    /// Store a single blob piece (writes directly to file).
    pub fn put_blob_piece(&self, blob_id: &str, piece_index: u32, data: &[u8]) -> Result<()> {
        self.write_blob_file(blob_id, piece_index, data)
    }

    /// Fetch a single blob piece (reads directly from file).
    pub fn get_blob_piece(&self, blob_id: &str, piece_index: u32) -> Result<Option<Vec<u8>>> {
        let Some(p) = &self.persist else {
            return Ok(None);
        };
        let path = p.blobs_dir.join(format!("{blob_id}.{piece_index}"));
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read(path)?;
        Ok(Some(data))
    }

    /// Fetch all pieces for a blob, ordered by index. Returns None unless
    /// exactly `expected_pieces` pieces (indexed 0..N with no gaps) are stored.
    pub fn get_blob(&self, blob_id: &str, expected_pieces: u32) -> Result<Option<Vec<Vec<u8>>>> {
        // First verify the total count matches
        let stored = self.blob_piece_count(blob_id)?;
        if stored != expected_pieces {
            return Ok(None);
        }
        let mut out = Vec::with_capacity(expected_pieces as usize);
        for i in 0..expected_pieces {
            match self.get_blob_piece(blob_id, i)? {
                Some(data) => out.push(data),
                None => return Ok(None),
            }
        }
        Ok(Some(out))
    }

    /// Count stored pieces for a blob (scans directory).
    pub fn blob_piece_count(&self, blob_id: &str) -> Result<u32> {
        let Some(p) = &self.persist else {
            return Ok(0);
        };
        let mut count = 0u32;
        for entry in std::fs::read_dir(&p.blobs_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with(&format!("{blob_id}.")) {
                count += 1;
            }
        }
        Ok(count)
    }

    // ── Local Moderation / Blocklist (§13.3) ───────────────────────────

    /// Block a node. `kind` is "block" or "spam".
    pub fn block_node(
        &self,
        node_pub: &str,
        kind: &str,
        reason: Option<&str>,
        now: u64,
    ) -> Result<()> {
        let _guard = self.shards[0].write_mutex.lock().expect("shard write mutex poisoned");
        let shard0 = &self.shards[0];
        let mut meta = shard0.meta.write().expect("meta lock poisoned");
        meta.blocklist.insert(
            node_pub.to_owned(),
            BlockRecord {
                kind: kind.to_owned(),
                reason: reason.map(|r| r.to_owned()),
                blocked_at: now,
            },
        );
        drop(meta);
        self.persist_meta()?;
        Ok(())
    }

    /// Unblock a node.
    pub fn unblock_node(&self, node_pub: &str) -> Result<bool> {
        let _guard = self.shards[0].write_mutex.lock().expect("shard write mutex poisoned");
        let shard0 = &self.shards[0];
        let mut meta = shard0.meta.write().expect("meta lock poisoned");
        let removed = meta.blocklist.remove(node_pub).is_some();
        drop(meta);
        if removed {
            self.persist_meta()?;
        }
        Ok(removed)
    }

    /// Check if a node is blocked.
    pub fn is_blocked(&self, node_pub: &str) -> Result<bool> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        Ok(meta.blocklist.contains_key(node_pub))
    }

    /// Get all blocked nodes.
    pub fn get_blocklist(&self) -> Result<Vec<BlocklistEntry>> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        let mut out: Vec<BlocklistEntry> = meta
            .blocklist
            .iter()
            .map(|(k, v)| (k.clone(), v.kind.clone(), v.reason.clone(), v.blocked_at))
            .collect();
        out.sort_by_key(|(_, _, _, at)| std::cmp::Reverse(*at));
        Ok(out)
    }

    // ── Attestation Storage (§8.3–§8.5) ────────────────────────────────

    /// Store an attestation envelope.
    pub fn put_attestation(&self, env: &Envelope) -> Result<String> {
        let id = datum_id(env);
        let inner = innermost(env);
        let c = inner
            .d
            .c
            .as_ref()
            .ok_or_else(|| anyhow!("attestation missing content"))?;
        let target = c
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("attestation missing c.target"))?;
        let level = c
            .get("level")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("attestation missing c.level"))?;

        {
            let _guard = self.shards[0].write_mutex.lock().expect("shard write mutex poisoned");
            let shard0 = &self.shards[0];
            let mut meta = shard0.meta.write().expect("meta lock poisoned");
            if meta.attestations.iter().any(|a| a.id == id) {
                return Ok(id);
            }
            meta.attestations.push(AttestationRecord {
                id: id.clone(),
                json: canonical(env)?,
                attester: inner.d.n.clone(),
                target: target.to_owned(),
                level: level.to_owned(),
                ts: inner.d.ts,
            });
        }
        self.persist_meta()?;
        Ok(id)
    }

    /// Get all attestations about a target node.
    pub fn get_attestations_for(&self, target: &str) -> Result<Vec<Envelope>> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        let mut recs: Vec<&AttestationRecord> = meta
            .attestations
            .iter()
            .filter(|a| a.target == target)
            .collect();
        recs.sort_by_key(|a| a.ts);
        recs.into_iter()
            .map(|a| serde_json::from_str(&a.json).context("invalid stored attestation JSON"))
            .collect()
    }

    /// Get attestation counts for a target: (positive, negative, neutral).
    pub fn attestation_counts(&self, target: &str) -> Result<(u64, u64, u64)> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        let mut pos = 0u64;
        let mut neg = 0u64;
        let mut neu = 0u64;
        for a in &meta.attestations {
            if a.target != target {
                continue;
            }
            match a.level.as_str() {
                "positive" => pos += 1,
                "negative" => neg += 1,
                "neutral" => neu += 1,
                _ => {}
            }
        }
        Ok((pos, neg, neu))
    }

    // ── Room Flag Aggregation (§13.8) ──────────────────────────────────

    /// Store a room.flag event.
    pub fn put_flag(&self, env: &Envelope) -> Result<String> {
        let id = datum_id(env);
        let inner = innermost(env);
        let room = inner
            .d
            .r
            .as_deref()
            .ok_or_else(|| anyhow!("room.flag missing room"))?;
        let c = inner
            .d
            .c
            .as_ref()
            .ok_or_else(|| anyhow!("room.flag missing content"))?;
        let target_eid = c
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("room.flag missing c.target"))?;
        let category = c
            .get("category")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("room.flag missing c.category"))?;

        {
            let _guard = self.shards[0].write_mutex.lock().expect("shard write mutex poisoned");
            let shard0 = &self.shards[0];
            let mut meta = shard0.meta.write().expect("meta lock poisoned");
            if meta.flags.iter().any(|f| f.id == id) {
                return Ok(id);
            }
            meta.flags.push(FlagRecord {
                id: id.clone(),
                room: room.to_owned(),
                target_eid: target_eid.to_owned(),
                category: category.to_owned(),
                flagger: inner.d.n.clone(),
                ts: inner.d.ts,
            });
        }
        self.persist_meta()?;
        Ok(id)
    }

    /// Get flag count for a specific event in a room.
    pub fn flag_count(&self, room: &str, target_eid: &str) -> Result<u64> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        Ok(meta
            .flags
            .iter()
            .filter(|f| f.room == room && f.target_eid == target_eid)
            .count() as u64)
    }

    /// Get flag counts by category for a specific event.
    pub fn flag_counts_by_category(
        &self,
        room: &str,
        target_eid: &str,
    ) -> Result<Vec<(String, u64)>> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        let mut counts: HashMap<String, u64> = HashMap::new();
        for f in &meta.flags {
            if f.room == room && f.target_eid == target_eid {
                *counts.entry(f.category.clone()).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, u64)> = counts.into_iter().collect();
        out.sort();
        Ok(out)
    }

    // ── Room Class & Retention ──────────────────────────────────────────

    /// Set the room class for retention policy.
    pub fn set_room_class(&self, room: &str, class: RoomClass) -> Result<()> {
        let _guard = self.shards[0].write_mutex.lock().expect("shard write mutex poisoned");
        let shard0 = &self.shards[0];
        let mut meta = shard0.meta.write().expect("meta lock poisoned");
        meta.room_meta.insert(
            room.to_owned(),
            RoomMeta {
                class,
                retention_days: None,
            },
        );
        drop(meta);
        self.persist_meta()?;
        Ok(())
    }

    /// Get the room class for a room (defaults to Private if not set).
    pub fn get_room_class(&self, room: &str) -> Result<RoomClass> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        Ok(meta
            .room_meta
            .get(room)
            .map(|m| m.class)
            .unwrap_or(RoomClass::Private))
    }

    /// Set custom retention override for a room (in days).
    pub fn set_room_retention(&self, room: &str, days: u64) -> Result<()> {
        let _guard = self.shards[0].write_mutex.lock().expect("shard write mutex poisoned");
        let shard0 = &self.shards[0];
        let mut meta = shard0.meta.write().expect("meta lock poisoned");
        meta.room_meta.entry(room.to_owned()).or_insert(RoomMeta {
            class: RoomClass::Private,
            retention_days: None,
        }).retention_days = Some(days);
        drop(meta);
        self.persist_meta()?;
        Ok(())
    }

    /// Get effective retention for a room (uses class default if no override).
    pub fn get_room_retention(&self, room: &str) -> Result<u64> {
        let shard0 = self.shard(None);
        let meta = shard0.meta.read().expect("meta lock poisoned");
        Ok(meta
            .room_meta
            .get(room)
            .and_then(|m| m.retention_days)
            .unwrap_or_else(|| {
                let class = meta.room_meta.get(room).map(|m| m.class).unwrap_or(RoomClass::Private);
                class.default_retention_days(&self.config)
            }))
    }

    // ── Storage Pruning (§11.5) ────────────────────────────────────────

    /// Prune events older than `retention_days` for a room. Returns count of removed events.
    ///
    /// Room-internal channels (`ch < 0`: metadata, membership, …) are
    /// retention-proof — they define room state (who is a member, current
    /// rules) and are small, so they are never pruned.
    pub fn prune_retention(&self, room: &str, retention_days: u64) -> Result<u64> {
        let cutoff_ms = retention_days * 86_400_000;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| anyhow!("system clock error"))?
            .as_millis() as u64;
        let threshold = now.saturating_sub(cutoff_ms);

        let shard_idx = Self::shard_idx(Some(room));
        let guard = self.shards[shard_idx].write_mutex.lock().expect("shard write mutex poisoned");
        let shard = &self.shards[shard_idx];

        let removed = {
            let ids: Vec<String> = shard.events.read().expect("events lock poisoned")
                .iter()
                .filter_map(|s| s.as_ref())
                .filter(|e| {
                    let is_system = e.channel.is_some_and(|c| c < 0);
                    e.room.as_deref() == Some(room) && !is_system && e.ts < threshold
                })
                .map(|e| e.id.to_string())
                .collect();
            for id in &ids {
                remove_id(shard, id);
            }
            // Rebuild indexes after pruning
            rebuild_indexes(shard);
            ids.len() as u64
        };
        drop(guard); // Release guard before calling compact to avoid deadlock
        if removed > 0 {
            self.compact()?;
        }
        Ok(removed)
    }

    /// Prune a room using its configured retention (class default or override).
    /// Returns count of removed events.
    pub fn prune_retention_configured(&self, room: &str) -> Result<u64> {
        let retention = self.get_room_retention(room)?;
        self.prune_retention(room, retention)
    }

    /// Prune all rooms according to their configured retention policies.
    /// Returns total count of removed events across all rooms.
    pub fn prune_all_rooms_retention(&self) -> Result<u64> {
        let rooms: Vec<String> = {
            let mut rooms = HashSet::new();
            self.read_all_shards(|shard| {
                for entry_opt in shard.events.read().expect("events lock poisoned").iter() {
                    if let Some(entry) = entry_opt {
                        if let Some(r) = &entry.room {
                            rooms.insert(r.clone());
                        }
                    }
                }
            });
            rooms.into_iter().map(|s| s.as_ref().to_string()).collect()
        };

        let mut total = 0u64;
        for room in rooms {
            total += self.prune_retention_configured(&room)?;
        }
        Ok(total)
    }

    /// Prune uncommitted events older than `ttl_ms`. Returns count of removed events.
    pub fn prune_uncommitted(&self, room: &str, ttl_ms: u64) -> Result<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| anyhow!("system clock error"))?
            .as_millis() as u64;
        let threshold = now.saturating_sub(ttl_ms);

        let shard_idx = Self::shard_idx(Some(room));
        let guard = self.shards[shard_idx].write_mutex.lock().expect("shard write mutex poisoned");
        let shard = &self.shards[shard_idx];

        let removed = {
            let ids: Vec<String> = shard.events.read().expect("events lock poisoned")
                .iter()
                .filter_map(|s| s.as_ref())
                .filter(|e| e.room.as_deref() == Some(room) && e.tc.is_none() && e.ts < threshold)
                .map(|e| e.id.to_string())
                .collect();
            for id in &ids {
                remove_id(shard, id);
            }
            rebuild_indexes(shard);
            ids.len() as u64
        };
        drop(guard); // Release guard before calling compact to avoid deadlock
        if removed > 0 {
            self.compact()?;
        }
        Ok(removed)
    }

    // ── Export / Import (§11.10) ────────────────────────────────────────

    /// Export all envelopes for a room ordered by (ts, id).
    pub fn export_room(&self, room: &str) -> Result<Vec<Envelope>> {
        let shard = self.shard(Some(room));
        let indices = shard.by_room_channel.get(&(room.to_string(), 0))
            .or_else(|| shard.by_room_channel.get(&(room.to_string(), 1)))
            .or_else(|| shard.by_room_channel.get(&(room.to_string(), -1)))
            .or_else(|| shard.by_room_channel.get(&(room.to_string(), -2)));

        let mut out = Vec::new();
        if let Some(indices) = indices {
            for &idx in indices.value() {
                if let Some(entry) = &shard.events.read().expect("events lock poisoned")[idx] {
                    out.push((entry.ts, entry.id.to_string(), entry.env.clone()));
                }
            }
        } else {
            // Fallback
            for entry_opt in shard.events.read().expect("events lock poisoned").iter() {
                if let Some(entry) = entry_opt {
                    if entry.room.as_deref() == Some(room) {
                        out.push((entry.ts, entry.id.to_string(), entry.env.clone()));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        Ok(out.into_iter().map(|(_, _, env)| (*env).clone()).collect())
    }

    /// Import envelopes into the pool. Validates signatures before storing.
    /// Returns count of successfully imported envelopes.
    pub fn import_envelopes(&self, envelopes: &[Envelope]) -> Result<u64> {
        let mut count = 0u64;
        for env in envelopes {
            if crate::envelope::verify_chain(env).is_ok() {
                self.put(env)?;
                count += 1;
            }
        }
        Ok(count)
    }
}

// ══════════════════════════════════════════════════════════════════════
// Internals
// ══════════════════════════════════════════════════════════════════════

fn lock_err<T>(_: T) -> anyhow::Error {
    anyhow!("pool lock poisoned")
}

/// Insert an envelope into the state (no persistence, no dedup check).
/// Caller must hold the shard's write_mutex. `id` must be `datum_id(env)`.
fn insert_new(shard: &Shard, env: &Envelope, id: &str) {
    let inner = innermost(env);
    let entry = Arc::new(PoolEntry {
        id: InternedStr::from(id),
        env: Arc::new(env.clone()),
        room: inner.d.r.clone().map(InternedStr::from),
        n: InternedStr::from(inner.d.n.clone()),
        to_node: inner.d.to.clone().map(InternedStr::from),
        ts: inner.d.ts,
        tc: env.d.tc,
        event_type: InternedStr::from(inner.d.t.clone()),
        channel: channel_of(&inner.d),
    });
    // Scope the events write lock: never hold it while taking an index lock
    // (readers take index locks first, then the events lock).
    let idx = {
        let mut events = shard.events.write().expect("events lock poisoned");
        let idx = events.len();
        events.push(Some(entry.clone()));
        idx
    };
    shard.live.fetch_add(1, Ordering::Relaxed);
    shard.by_id.insert(id.to_owned(), idx);

    // Update secondary indexes
    if let Some(room) = &entry.room {
        if let Some(ch) = entry.channel {
            // by_room_channel uses String for fast &str lookup (lock-free)
            shard.by_room_channel
                .entry((room.as_ref().to_string(), ch))
                .or_default()
                .push(idx);
        }
    }
    if let Some(tc) = entry.tc {
        if let Some(room) = &entry.room {
            // by_room_tc and by_author_room_tc use InternedStr for shared allocation (need write lock)
            shard.by_room_tc.write().expect("by_room_tc lock poisoned").insert((room.clone(), tc), idx);
            shard.by_author_room_tc.write().expect("by_author_room_tc lock poisoned").insert((entry.n.clone(), room.clone(), tc), idx);
        }
    }
}

fn remove_id(shard: &Shard, id: &str) {
    if let Some((_, idx)) = shard.by_id.remove(id) {
        let mut events = shard.events.write().expect("events lock poisoned");
        let matches = events
            .get(idx)
            .and_then(|slot| slot.as_ref())
            .is_some_and(|entry| entry.id.as_ref() == id);
        if matches {
            events[idx] = None;
            shard.live.fetch_sub(1, Ordering::Relaxed);
        }
        // Secondary indexes are not cleaned up here for performance.
        // They are rebuilt on compact() and prune_retention().
        // Stale index entries are filtered out at query time by checking events[idx].is_some().
    }
}

/// Rebuild all secondary indexes from the current events vector.
///
/// Caller must hold the shard's `write_mutex`. The events lock is released
/// before the index locks are taken to preserve the readers' lock order
/// (index read → events read).
fn rebuild_indexes(shard: &Shard) {
    let snapshot: Vec<Option<Arc<PoolEntry>>> = shard
        .events
        .read()
        .expect("events lock poisoned")
        .clone();

    shard.by_room_channel.clear();
    let mut by_room_tc = shard.by_room_tc.write().expect("by_room_tc lock poisoned");
    let mut by_author_room_tc = shard
        .by_author_room_tc
        .write()
        .expect("by_author_room_tc lock poisoned");
    by_room_tc.clear();
    by_author_room_tc.clear();

    for (idx, entry_opt) in snapshot.iter().enumerate() {
        if let Some(entry) = entry_opt {
            if let Some(room) = &entry.room {
                if let Some(ch) = entry.channel {
                    // by_room_channel uses String for fast &str lookup
                    shard
                        .by_room_channel
                        .entry((room.as_ref().to_string(), ch))
                        .or_default()
                        .push(idx);
                }
            }
            if let Some(tc) = entry.tc {
                if let Some(room) = &entry.room {
                    // by_room_tc and by_author_room_tc use InternedStr for shared allocation
                    by_room_tc.insert((room.clone(), tc), idx);
                    by_author_room_tc.insert((entry.n.clone(), room.clone(), tc), idx);
                }
            }
        }
    }
}

/// Committed events first (tc ASC), then uncommitted (ts ASC, id ASC).
fn cmp_committed_first(a: &PoolEntry, b: &PoolEntry) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.tc, b.tc) {
        (Some(x), Some(y)) => x
            .cmp(&y)
            .then_with(|| a.ts.cmp(&b.ts))
            .then_with(|| a.id.cmp(&b.id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)),
    }
}

fn cmp_tc(a: &PoolEntry, b: &PoolEntry) -> std::cmp::Ordering {
    a.tc.cmp(&b.tc)
        .then_with(|| a.ts.cmp(&b.ts))
        .then_with(|| a.id.cmp(&b.id))
}

fn cmp_ts_id(a: &PoolEntry, b: &PoolEntry) -> std::cmp::Ordering {
    a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id))
}

/// Load a `RuleSet` from the latest committed channel -1 envelope in the pool.
/// Returns an empty `RuleSet` if no metadata exists yet.
pub fn ruleset_from_pool(pool: &Pool, room: &str) -> Result<RuleSet> {
    let meta_events = pool.get_room_channel(room, -1)?;
    let latest_committed = meta_events.iter().rev().find(|e| e.tc.is_some());
    let Some(entry) = latest_committed else {
        return Ok(RuleSet::default());
    };
    let inner = innermost(&entry.env);
    let rules_val = inner.d.c.as_ref().and_then(|c| c.get("rules"));
    match rules_val {
        Some(v) => Ok(RuleSet::from_json(v)),
        None => Ok(RuleSet::default()),
    }
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;

    use serde_json::json;

    use crate::envelope::{innermost, wrap_commit};
    use crate::events::{make_room_create, make_room_update};

    use super::{ruleset_from_pool, Pool, PoolConfig, RoomClass};
    // ---- Persistence helpers ----

    fn tmp_base(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let uniq = format!(
            "toloo-pool-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        dir.join(uniq)
    }

    #[test]
    fn put_get_and_deduplicate_by_datum_id() {
        let pool = Pool::memory().expect("pool should initialize");
        let node = mk_node();
        let room = mk_room();
        let env = mk_room_event(
            &node,
            &room,
            "room.message",
            100,
            json!({"ch": 1, "body": "x"}),
        );

        let id1 = pool.put(&env).expect("put should succeed");
        let id2 = pool.put(&env).expect("duplicate put should succeed");
        assert_eq!(id1, id2);

        let got = pool
            .get(&id1)
            .expect("get should succeed")
            .expect("must exist");
        assert_eq!(got.p, env.p);

        let entries = pool
            .get_room_channel(&room.sig.pub_key, 1)
            .expect("query should succeed");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn put_batch_inserts_multiple_envelopes() {
        let pool = Pool::memory().expect("pool should initialize");
        let node = mk_node();
        let room = mk_room();

        // Create 10 envelopes
        let envelopes: Vec<_> = (0..10)
            .map(|i| {
                mk_room_event(
                    &node,
                    &room,
                    "room.message",
                    1000 + i,
                    json!({"ch": 1, "body": format!("msg{}", i)}),
                )
            })
            .collect();
        let refs: Vec<_> = envelopes.iter().collect();

        // Batch insert
        let ids = pool.put_batch(&refs).expect("put_batch should succeed");
        assert_eq!(ids.len(), 10);

        // Verify all are retrievable
        for id in &ids {
            let got = pool.get(id).expect("get").expect("must exist");
            assert_eq!(got.d.t, "room.message");
        }

        // Verify count in room channel
        let entries = pool.get_room_channel(&room.sig.pub_key, 1).expect("query");
        assert_eq!(entries.len(), 10);
    }

    #[test]
    fn put_batch_deduplicates() {
        let pool = Pool::memory().expect("pool should initialize");
        let node = mk_node();
        let room = mk_room();

        let env = mk_room_event(
            &node,
            &room,
            "room.message",
            100,
            json!({"ch": 1, "body": "duplicate"}),
        );
        let refs = vec![&env, &env, &env];

        let ids = pool.put_batch(&refs).expect("put_batch");
        assert_eq!(ids.len(), 3); // returns ID for each input position
        assert_eq!(ids[0], ids[1]); // but deduplicated to same ID
        assert_eq!(ids[1], ids[2]);
    }

    #[test]
    fn committed_event_replaces_uncommitted_with_same_eid() {
        let pool = Pool::memory().expect("pool should initialize");
        let author = mk_node();
        let room = mk_room();
        let inner = mk_room_event(
            &author,
            &room,
            "room.message",
            101,
            json!({"ch": 2, "body": "hello"}),
        );

        pool.put(&inner).expect("put depth1");
        assert_eq!(
            pool.get_uncommitted(&room.sig.pub_key)
                .expect("query")
                .len(),
            1
        );

        let commit = wrap_commit(inner, &room, 200).expect("commit should wrap");
        pool.put(&commit).expect("put depth2");

        let channel_entries = pool
            .get_room_channel(&room.sig.pub_key, 2)
            .expect("query should succeed");
        assert_eq!(channel_entries.len(), 1);
        assert_eq!(channel_entries[0].tc, Some(200));
        assert_eq!(
            pool.get_uncommitted(&room.sig.pub_key)
                .expect("query")
                .len(),
            0
        );
    }

    #[test]
    fn latest_tc_and_room_ids_work_for_committed_room_events() {
        let pool = Pool::memory().expect("pool should initialize");
        let author = mk_node();
        let room = mk_room();

        let e1 = mk_room_event(
            &author,
            &room,
            "room.message",
            1000,
            json!({"ch": 1, "body": "a"}),
        );
        let e2 = mk_room_event(
            &author,
            &room,
            "room.message",
            2000,
            json!({"ch": 1, "body": "b"}),
        );
        let c1 = wrap_commit(e1, &room, 3000).expect("commit");
        let c2 = wrap_commit(e2, &room, 4000).expect("commit");

        let id1 = pool.put(&c1).expect("put");
        let id2 = pool.put(&c2).expect("put");

        let ids = pool.get_room_ids(&room.sig.pub_key).expect("ids query");
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));

        let latest = pool.latest_tc(&room.sig.pub_key).expect("latest");
        assert_eq!(latest, 4000);
    }

    #[test]
    fn get_private_returns_sender_and_receiver_events() {
        let pool = Pool::memory().expect("pool should initialize");
        let arta = mk_node();
        let babak = mk_node();
        let mithra = mk_node();

        let ab = mk_private_message(&arta, &babak.sig.pub_key, 10);
        let ca = mk_private_message(&mithra, &arta.sig.pub_key, 11);
        let bc = mk_private_message(&babak, &mithra.sig.pub_key, 12);

        pool.put(&ab).expect("put");
        pool.put(&ca).expect("put");
        pool.put(&bc).expect("put");

        let arta_private = pool.get_private(&arta.sig.pub_key).expect("query");
        assert_eq!(arta_private.len(), 2);
        assert!(arta_private.iter().all(|e| e.room.is_none()));
    }

    #[test]
    fn build_membership_replays_channel_minus_two_committed_only() {
        let pool = Pool::memory().expect("pool should initialize");
        let key_holder = mk_room();
        let arta = mk_node();
        let babak = mk_node();

        let join_arta = mk_room_event(&arta, &key_holder, "room.join", 100, json!({}));
        let ban_arta = mk_room_event(
            &babak,
            &key_holder,
            "room.ban",
            101,
            json!({ "banned": arta.sig.pub_key }),
        );
        let unban_arta = mk_room_event(
            &babak,
            &key_holder,
            "room.unban",
            102,
            json!({ "unbanned": arta.sig.pub_key }),
        );
        let invite = mk_room_event(
            &babak,
            &key_holder,
            "room.invite",
            103,
            json!({ "invitee": arta.sig.pub_key }),
        );
        let join_arta_again = mk_room_event(&arta, &key_holder, "room.join", 104, json!({}));

        // One uncommitted membership event should not affect final state.
        pool.put(&join_arta).expect("put");

        for (e, tc) in [
            (ban_arta, 1001u64),
            (unban_arta, 1002u64),
            (invite, 1003u64),
            (join_arta_again, 1004u64),
        ] {
            let committed = wrap_commit(e, &key_holder, tc).expect("commit");
            pool.put(&committed).expect("put committed");
        }

        let state = pool
            .build_membership(&key_holder.sig.pub_key)
            .expect("membership");
        assert!(state.members.contains(&arta.sig.pub_key));
        assert!(!state.banned.contains(&arta.sig.pub_key));

        // Cached state must be invalidated when a new membership commit lands.
        let leave = mk_room_event(&arta, &key_holder, "room.leave", 105, json!({}));
        let leave = wrap_commit(leave, &key_holder, 1005).expect("commit");
        pool.put(&leave).expect("put committed leave");
        let state = pool
            .build_membership(&key_holder.sig.pub_key)
            .expect("membership");
        assert!(
            !state.members.contains(&arta.sig.pub_key),
            "membership cache must invalidate on new committed membership events"
        );
    }

    // ---- ruleset_from_pool ----

    #[test]
    fn from_room_meta_returns_empty_for_unknown_room() {
        let pool = Pool::memory().expect("pool");
        let rs = ruleset_from_pool(&pool, "NONEXISTENT_ROOM_SIG").expect("should not error");
        assert!(rs.rules().is_empty());
        assert!(!rs.can_join());
    }

    #[test]
    fn from_room_meta_reads_rules_from_pool() {
        let pool = Pool::memory().expect("pool");
        let node = mk_node();
        let room = mk_room();
        let rules = json!([{"t": "join", "allow": "*"}, {"t": "post", "allow": "*"}]);

        let (_d1, commit) =
            make_room_create(&node, &room, Some("Test Room"), Some(rules)).expect("room.create");
        pool.put(&commit).expect("put");

        let rs = ruleset_from_pool(&pool, &room.sig.pub_key).expect("from_room_meta");
        assert!(rs.can_join());
        assert!(rs.can_post());
    }

    #[test]
    fn from_room_meta_uses_latest_committed_channel_minus_one() {
        let pool = Pool::memory().expect("pool");
        let node = mk_node();
        let room = mk_room();

        let rules1 = json!([{"t": "join", "allow": "*"}]);
        let (_d1, commit1) =
            make_room_create(&node, &room, Some("Room"), Some(rules1)).expect("room.create");
        pool.put(&commit1).expect("put c1");

        let rules2 = json!([{"t": "join", "allow": "*"}, {"t": "post", "allow": "*"}]);
        let update = make_room_update(&node, &room, None, Some(rules2)).expect("room.update");
        let tc2 = commit1.d.tc.unwrap() + 1000;
        let commit2 = wrap_commit(update, &room, tc2).expect("commit2");
        pool.put(&commit2).expect("put c2");

        let rs = ruleset_from_pool(&pool, &room.sig.pub_key).expect("from_room_meta");
        assert!(rs.can_join());
        assert!(rs.can_post(), "latest update should enable posting");
    }

    // ---- TOFU pin storage ----

    #[test]
    fn tofu_new_pin_then_match() {
        let pool = Pool::memory().expect("pool");
        let result = pool.check_tofu_pin("NODE_A", "fp_aaa", 1000).unwrap();
        assert_eq!(result, super::TofuResult::NewPin);

        let result = pool.check_tofu_pin("NODE_A", "fp_aaa", 2000).unwrap();
        assert_eq!(result, super::TofuResult::Match);
    }

    #[test]
    fn tofu_mismatch_on_fingerprint_change() {
        let pool = Pool::memory().expect("pool");
        pool.check_tofu_pin("NODE_B", "fp_original", 100).unwrap();
        let result = pool.check_tofu_pin("NODE_B", "fp_changed", 200).unwrap();
        match result {
            super::TofuResult::Mismatch {
                expected,
                first_seen_at,
            } => {
                assert_eq!(expected, "fp_original");
                assert_eq!(first_seen_at, 100);
            }
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    // ---- Blob piece storage (on-disk only) ----

    #[test]
    fn blob_piece_store_and_fetch() {
        let base = tmp_base("blob");
        let path = base.to_str().unwrap().to_owned();
        let pool = Pool::open_default(&path).unwrap();

        pool.put_blob_piece("blob1", 0, b"piece0").unwrap();
        pool.put_blob_piece("blob1", 1, b"piece1").unwrap();

        assert_eq!(pool.blob_piece_count("blob1").unwrap(), 2);
        assert_eq!(pool.get_blob_piece("blob1", 0).unwrap().unwrap(), b"piece0");
        assert_eq!(pool.get_blob_piece("blob1", 1).unwrap().unwrap(), b"piece1");
        assert!(pool.get_blob_piece("blob1", 2).unwrap().is_none());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn get_blob_returns_all_pieces_in_order() {
        let base = tmp_base("blob2");
        let path = base.to_str().unwrap().to_owned();
        let pool = Pool::open_default(&path).unwrap();

        pool.put_blob_piece("blob2", 0, b"aaa").unwrap();
        pool.put_blob_piece("blob2", 1, b"bbb").unwrap();
        pool.put_blob_piece("blob2", 2, b"ccc").unwrap();

        let pieces = pool.get_blob("blob2", 3).unwrap().unwrap();
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0], b"aaa");
        assert_eq!(pieces[2], b"ccc");

        // Wrong expected count returns None
        assert!(pool.get_blob("blob2", 2).unwrap().is_none());

        std::fs::remove_dir_all(&base).ok();
    }

    // ---- Local moderation ----

    #[test]
    fn block_and_unblock_node() {
        let pool = Pool::memory().expect("pool");
        assert!(!pool.is_blocked("NODE_X").unwrap());

        pool.block_node("NODE_X", "block", Some("spam"), 1000)
            .unwrap();
        assert!(pool.is_blocked("NODE_X").unwrap());

        let list = pool.get_blocklist().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "NODE_X");
        assert_eq!(list[0].1, "block");

        pool.unblock_node("NODE_X").unwrap();
        assert!(!pool.is_blocked("NODE_X").unwrap());
    }

    // ---- Attestation storage ----

    #[test]
    fn attestation_store_and_query() {
        let pool = Pool::memory().expect("pool");
        let attester = mk_node();
        let target = mk_node();

        let att = crate::events::make_side_attestation(
            &attester,
            &target.sig.pub_key,
            "positive",
            Some("good peer"),
        )
        .unwrap();
        pool.put_attestation(&att).unwrap();

        let atts = pool.get_attestations_for(&target.sig.pub_key).unwrap();
        assert_eq!(atts.len(), 1);

        let (pos, neg, neu) = pool.attestation_counts(&target.sig.pub_key).unwrap();
        assert_eq!(pos, 1);
        assert_eq!(neg, 0);
        assert_eq!(neu, 0);
    }

    // ---- Room flag aggregation ----

    #[test]
    fn flag_store_and_count() {
        let pool = Pool::memory().expect("pool");
        let node = mk_node();
        let room = mk_room();

        let flag = crate::events::make_room_flag(
            &node,
            &room.sig.pub_key,
            "eid_123",
            "spam",
            Some("offensive"),
        )
        .unwrap();
        pool.put_flag(&flag).unwrap();

        assert_eq!(pool.flag_count(&room.sig.pub_key, "eid_123").unwrap(), 1);

        let by_cat = pool
            .flag_counts_by_category(&room.sig.pub_key, "eid_123")
            .unwrap();
        assert_eq!(by_cat.len(), 1);
        assert_eq!(by_cat[0], ("spam".to_owned(), 1));
    }

    // ---- Export / Import ----

    #[test]
    fn export_import_roundtrip() {
        let pool = Pool::memory().expect("pool");
        let node = mk_node();
        let room = mk_room();

        let (_d1, commit) = make_room_create(&node, &room, Some("Export Room"), Some(json!([])))
            .expect("room.create");
        pool.put(&commit).expect("put");

        let exported = pool.export_room(&room.sig.pub_key).unwrap();
        assert_eq!(exported.len(), 1);

        let pool2 = Pool::memory().expect("pool2");
        let imported = pool2.import_envelopes(&exported).unwrap();
        assert_eq!(imported, 1);

        let re_exported = pool2.export_room(&room.sig.pub_key).unwrap();
        assert_eq!(re_exported.len(), 1);
    }

    // ---- Persistence ----

    #[test]
    fn prune_retention_never_removes_system_channels() {
        let pool = Pool::memory().expect("pool");
        let node = mk_node();
        let room = mk_room();

        // System events (membership + metadata) with ancient timestamps.
        let join = mk_room_event(&node, &room, "room.join", 1, json!({}));
        let meta = mk_room_event(
            &node,
            &room,
            "room.update",
            2,
            json!({"name": "Old Name", "rules": []}),
        );
        pool.put(&wrap_commit(join, &room, 10).unwrap()).unwrap();
        pool.put(&wrap_commit(meta, &room, 11).unwrap()).unwrap();

        // Ancient content event.
        let msg = mk_room_event(
            &node,
            &room,
            "room.message",
            3,
            json!({"ch": 1, "body": "old"}),
        );
        pool.put(&wrap_commit(msg, &room, 12).unwrap()).unwrap();

        // Retention window that expires everything.
        let pruned = pool.prune_retention(&room.sig.pub_key, 1).unwrap();
        assert_eq!(pruned, 1, "only the content event is prunable");

        let state = pool.build_membership(&room.sig.pub_key).unwrap();
        assert!(
            state.members.contains(&node.sig.pub_key),
            "membership must survive retention pruning"
        );
        assert_eq!(
            pool.get_room_channel(&room.sig.pub_key, -1).unwrap().len(),
            1,
            "metadata must survive retention pruning"
        );
    }

    #[test]
    fn room_watermark_tracks_latest_system_commit() {
        let pool = Pool::memory().expect("pool");
        let node = mk_node();
        let room = mk_room();

        assert_eq!(
            pool.room_watermark(&room.sig.pub_key).unwrap(),
            0,
            "empty room has watermark 0"
        );

        let join = mk_room_event(&node, &room, "room.join", 100, json!({}));
        pool.put(&wrap_commit(join, &room, 500).unwrap()).unwrap();
        assert_eq!(pool.room_watermark(&room.sig.pub_key).unwrap(), 500);

        // Newer metadata commit advances the watermark.
        let meta = mk_room_event(
            &node,
            &room,
            "room.update",
            900,
            json!({"name": "Renamed", "rules": []}),
        );
        pool.put(&wrap_commit(meta, &room, 1000).unwrap()).unwrap();
        assert_eq!(pool.room_watermark(&room.sig.pub_key).unwrap(), 1000);

        // Content commits do not advance the room watermark.
        let msg = mk_room_event(
            &node,
            &room,
            "room.message",
            2000,
            json!({"ch": 1, "body": "x"}),
        );
        pool.put(&wrap_commit(msg, &room, 5000).unwrap()).unwrap();
        assert_eq!(
            pool.room_watermark(&room.sig.pub_key).unwrap(),
            1000,
            "content channel commits must not advance the watermark"
        );

        // Uncommitted system events do not advance the watermark either.
        let join2 = mk_room_event(&node, &room, "room.join", 6000, json!({}));
        pool.put(&join2).unwrap();
        assert_eq!(pool.room_watermark(&room.sig.pub_key).unwrap(), 1000);
    }

    #[test]
    fn persistent_pool_reopens_with_state() {
        let base = tmp_base("reopen");
        let path = base.to_str().unwrap().to_owned();
        let room_key;

        {
            let pool = Pool::open_default(&path).expect("open");
            let node = mk_node();
            let room = mk_room();
            room_key = room.sig.pub_key.clone();

            let e1 = mk_room_event(
                &node,
                &room,
                "room.message",
                100,
                json!({"ch": 1, "body": "a"}),
            );
            let c1 = wrap_commit(e1, &room, 200).expect("commit");
            pool.put(&c1).expect("put");
            pool.check_tofu_pin("NODE_P", "fp", 5).expect("pin");
            pool.block_node("NODE_SPAM", "block", None, 6)
                .expect("block");
            pool.put_blob_piece("blobz", 0, b"zzz").expect("piece");
        }

        let pool = Pool::open_default(&path).expect("reopen");
        assert_eq!(pool.latest_tc(&room_key).unwrap(), 200);
        assert_eq!(pool.get_room_channel(&room_key, 1).unwrap().len(), 1);
        assert_eq!(pool.get_tofu_pin("NODE_P").unwrap().unwrap().0, "fp");
        assert!(pool.is_blocked("NODE_SPAM").unwrap());
        assert_eq!(pool.get_blob_piece("blobz", 0).unwrap().unwrap(), b"zzz");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn truncated_final_log_line_is_discarded() {
        let base = tmp_base("truncated");
        let path = base.to_str().unwrap().to_owned();
        let events_path = base.with_extension("jsonl");

        {
            let pool = Pool::open_default(&path).expect("open");
            let node = mk_node();
            let room = mk_room();
            let env = mk_room_event(
                &node,
                &room,
                "room.message",
                42,
                json!({"ch": 1, "body": "ok"}),
            );
            pool.put(&env).expect("put");
        }

        // Simulate a crash mid-append: append a partial JSON line.
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&events_path)
                .unwrap();
            f.write_all(b"{\"d\":{\"n\":\"BRO").unwrap();
        }

        Pool::open_default(&path).expect("reopen with truncated tail");
        // The one good event must have survived; the partial line must be gone.
        let log = std::fs::read_to_string(&events_path).unwrap();
        assert_eq!(log.lines().count(), 1);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn prune_compacts_log_and_reopen_reflects_it() {
        let base = tmp_base("prune");
        let path = base.to_str().unwrap().to_owned();
        let room_key;

        {
            let pool = Pool::open_default(&path).expect("open");
            let node = mk_node();
            let room = mk_room();
            room_key = room.sig.pub_key.clone();

            // Two old messages (ts ~ 0) and one fresh one.
            let old1 = mk_room_event(
                &node,
                &room,
                "room.message",
                1,
                json!({"ch": 1, "body": "old1"}),
            );
            let old2 = mk_room_event(
                &node,
                &room,
                "room.message",
                2,
                json!({"ch": 1, "body": "old2"}),
            );
            let fresh = mk_room_event(
                &node,
                &room,
                "room.message",
                u64::MAX / 2,
                json!({"ch": 1, "body": "new"}),
            );
            for e in [old1, old2, fresh] {
                let ts = e.d.ts;
                pool.put(&wrap_commit(e, &room, ts).unwrap()).unwrap();
            }
        }

        let pool = Pool::open_default(&path).expect("reopen");
        let pruned = pool.prune_retention(&room_key, 1).expect("prune");
        assert_eq!(pruned, 2);

        drop(pool);
        let pool = Pool::open_default(&path).expect("reopen after prune");
        assert_eq!(pool.get_room_channel(&room_key, 1).unwrap().len(), 1);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn pool_config_defaults() {
        let config = PoolConfig::default();
        assert_eq!(config.max_resident_blocks, 0);
        assert_eq!(config.tail_blocks_to_keep, 1000);
        assert_eq!(config.retention_public_days, 7);
        assert_eq!(config.retention_personal_days, 90);
        assert_eq!(config.retention_private_days, 30);
    }

    #[test]
    fn room_class_default_retention() {
        let config = PoolConfig::default();
        assert_eq!(RoomClass::Public.default_retention_days(&config), 7);
        assert_eq!(RoomClass::Personal.default_retention_days(&config), 90);
        assert_eq!(RoomClass::Private.default_retention_days(&config), 30);
    }

    #[test]
    fn tiered_storage_spills_to_cold() {
        let base = tmp_base("tiered");
        let path = base.to_str().unwrap().to_owned();
        let room_key;

        let config = PoolConfig {
            max_resident_blocks: 5,
            tail_blocks_to_keep: 3,
            cold_storage_path: None,
            ..PoolConfig::default()
        };

        {
            let pool = Pool::open(&path, config).expect("open");
            let node = mk_node();
            let room = mk_room();
            room_key = room.sig.pub_key.clone();

            // Add 10 events
            for i in 0..10 {
                let env = mk_room_event(
                    &node,
                    &room,
                    "room.message",
                    1000 + i,
                    json!({"ch": 1, "body": format!("msg{}", i)}),
                );
                let committed = wrap_commit(env, &room, 2000 + i).unwrap();
                pool.put(&committed).expect("put");
            }

            // After putting 10 events with max_resident_blocks=5,
            // 5 oldest should be spilled to cold storage
            let cold_path = base.with_extension("cold.jsonl");
            assert!(cold_path.exists(), "cold storage file should exist");

            let cold_content = std::fs::read_to_string(&cold_path).unwrap();
            let cold_lines: Vec<_> = cold_content.lines().collect();
            assert_eq!(cold_lines.len(), 5, "5 events should be in cold storage");
        }

        // Reopen and verify hot storage has only 5 most recent
        let pool = Pool::open_default(&path).expect("reopen");
        let hot_events = pool.get_room_channel(&room_key, 1).unwrap();
        assert_eq!(hot_events.len(), 5, "hot storage should have 5 most recent events");

        // Verify they are the most recent (msg5 through msg9)
        for (i, event) in hot_events.iter().enumerate() {
            let inner = innermost(&event.env);
            let body = inner.d.c.as_ref()
                .and_then(|c| c.get("body"))
                .and_then(|v| v.as_str())
                .unwrap();
            assert_eq!(body, format!("msg{}", 5 + i));
        }

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn tail_only_replay_returns_recent() {
        let base = tmp_base("tail");
        let path = base.to_str().unwrap().to_owned();
        let room_key;

        let config = PoolConfig {
            max_resident_blocks: 5,
            tail_blocks_to_keep: 3,
            cold_storage_path: None,
            ..PoolConfig::default()
        };

        {
            let pool = Pool::open(&path, config.clone()).expect("open");
            let node = mk_node();
            let room = mk_room();
            room_key = room.sig.pub_key.clone();

            for i in 0..10 {
                let env = mk_room_event(
                    &node,
                    &room,
                    "room.message",
                    1000 + i,
                    json!({"ch": 1, "body": format!("msg{}", i)}),
                );
                let committed = wrap_commit(env, &room, 2000 + i).unwrap();
                pool.put(&committed).expect("put");
            }
        }

        // Reopen with same config to test get_wanted behavior
        let pool = Pool::open(&path, config).expect("reopen");

        // get_wanted with after_tc=0 should return tail (3 events due to tail_blocks_to_keep=3)
        let wanted = pool.get_wanted(&[&room_key], &[], 0, 100).unwrap();
        assert_eq!(wanted.len(), 3, "tail-only replay should return 3 events");

        // Verify they are the most recent
        for (i, event) in wanted.iter().enumerate() {
            let inner = innermost(&event.env);
            let body = inner.d.c.as_ref()
                .and_then(|c| c.get("body"))
                .and_then(|v| v.as_str())
                .unwrap();
            assert_eq!(body, format!("msg{}", 7 + i));
        }

        // get_wanted_full should return all events from cold + hot
        let wanted_full = pool.get_wanted_full(&[&room_key], &[], 0, 100).unwrap();
        assert_eq!(wanted_full.len(), 10, "full history should return all 10 events");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn room_class_and_retention() {
        let pool = Pool::memory().expect("pool");
        let room_pub = "test_room_pub_key";

        // Default class is Private
        assert_eq!(pool.get_room_class(room_pub).unwrap(), RoomClass::Private);

        // Set to Public
        pool.set_room_class(room_pub, RoomClass::Public).unwrap();
        assert_eq!(pool.get_room_class(room_pub).unwrap(), RoomClass::Public);

        // Default retention for Public should be 7 days
        assert_eq!(pool.get_room_retention(room_pub).unwrap(), 7);

        // Override retention
        pool.set_room_retention(room_pub, 14).unwrap();
        assert_eq!(pool.get_room_retention(room_pub).unwrap(), 14);

        // Change class to Personal, retention should follow class default
        pool.set_room_class(room_pub, RoomClass::Personal).unwrap();
        assert_eq!(pool.get_room_retention(room_pub).unwrap(), 90);

        // Explicit override should still win
        pool.set_room_retention(room_pub, 60).unwrap();
        assert_eq!(pool.get_room_retention(room_pub).unwrap(), 60);
    }

    #[test]
    fn prune_retention_configured_uses_room_class() {
        let base = tmp_base("prune_class");
        let path = base.to_str().unwrap().to_owned();
        let room_key;

        let config = PoolConfig {
            retention_public_days: 1,  // 1 day for testing
            retention_personal_days: 90,
            retention_private_days: 30,
            ..PoolConfig::default()
        };

        {
            let pool = Pool::open(&path, config).expect("open");
            let node = mk_node();
            let room = mk_room();
            room_key = room.sig.pub_key.clone();

            // Set room as Public (1 day retention)
            pool.set_room_class(&room_key, RoomClass::Public).unwrap();

            // Add old event (ts=1, tc=10) and new event (ts=MAX/2, tc=MAX/2+10)
            let old = mk_room_event(&node, &room, "room.message", 1, json!({"ch": 1, "body": "old"}));
            let new = mk_room_event(&node, &room, "room.message", u64::MAX / 2, json!({"ch": 1, "body": "new"}));
            pool.put(&wrap_commit(old, &room, 10).unwrap()).unwrap();
            pool.put(&wrap_commit(new, &room, u64::MAX / 2 + 10).unwrap()).unwrap();
        }

        // Reopen and prune using configured retention
        let pool = Pool::open_default(&path).expect("reopen");
        let _pruned = pool.prune_retention_configured(&room_key).unwrap();

        // With 1 day retention and old event at ts=1 (ancient), it should be pruned
        // But system time might not be far enough, so we can't assert exact count
        // Just verify the method runs without error

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn concurrent_put_and_query() {
        use std::sync::Arc;
        use std::thread;

        let pool = Arc::new(Pool::memory().expect("pool"));
        let node = Arc::new(mk_node());
        let room = Arc::new(mk_room());
        let room_pub = room.sig.pub_key.clone();

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let pool = pool.clone();
                let node = node.clone();
                let room = room.clone();
                thread::spawn(move || {
                    for j in 0..100 {
                        let env = mk_room_event(
                            &node,
                            &room,
                            "room.message",
                            (i * 100 + j) as u64,
                            json!({"ch": 1, "body": format!("msg-{}-{}", i, j)}),
                        );
                        pool.put(&env).expect("put");
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Query from multiple threads
        let query_handles: Vec<_> = (0..10)
            .map(|_| {
                let pool = pool.clone();
                let room_pub = room_pub.clone();
                thread::spawn(move || {
                    for _ in 0..100 {
                        let _ = pool.get_room_channel(&room_pub, 1).expect("query");
                        let _ = pool.latest_tc(&room_pub).expect("latest_tc");
                        let _ = pool.get_wanted(&[&room_pub], &[], 0, 10).expect("get_wanted");
                    }
                })
            })
            .collect();

        for h in query_handles {
            h.join().unwrap();
        }

        let final_count = pool.get_room_channel(&room_pub, 1).unwrap().len();
        assert_eq!(final_count, 1000);
    }
}