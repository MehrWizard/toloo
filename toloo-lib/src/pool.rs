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
//! - `X.blobs/` — blob piece files, one per piece (write-once).
//!
//! Superseded and pruned events stay in the log until [`Pool::compact`]
//! rewrites it from memory. Pruning methods compact automatically.
//!
//! [`Pool::memory`] runs the same engine with no persistence at all.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{LockResult, RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{anyhow, Context, Result};

use crate::canonical::canonical;
use crate::envelope::{depth, innermost};
use crate::ids::{channel_of, datum_id};
use crate::rules::RuleSet;
use crate::types::Envelope;

/// A blocklist row: (node_pub, kind, reason, blocked_at).
pub type BlocklistEntry = (String, String, Option<String>, u64);

pub struct Pool {
    state: RwLock<PoolState>,
    persist: Option<Persist>,
}

/// Where this pool persists, derived from the base path given to [`Pool::open`].
struct Persist {
    events_path: PathBuf,
    meta_path: PathBuf,
    blobs_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PoolEntry {
    pub id: String,
    pub env: Envelope,
    pub room: Option<String>,
    pub n: String,
    pub to_node: Option<String>,
    pub ts: u64,
    pub tc: Option<u64>,
    pub event_type: String,
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
}

#[derive(Default)]
struct PoolState {
    /// Live events in insertion order; `None` marks a removed slot.
    events: Vec<Option<PoolEntry>>,
    by_id: HashMap<String, usize>,
    meta: MetaState,
    /// In-memory mirror of blob pieces (persisted piece files are scanned on open).
    blob_pieces: HashMap<(String, u32), Vec<u8>>,
}

impl Pool {
    /// Open a persistent pool. `path` is a base path; the event log lives at
    /// `<path>.jsonl` alongside `<path>.meta.json` and `<path>.blobs/`.
    pub fn open(path: &str) -> Result<Self> {
        let base = Path::new(path);
        let persist = Persist {
            events_path: base.with_extension("jsonl"),
            meta_path: base.with_extension("meta.json"),
            blobs_dir: base.with_extension("blobs"),
        };
        std::fs::create_dir_all(&persist.blobs_dir)
            .with_context(|| format!("failed to create blob dir {}", persist.blobs_dir.display()))?;

        let pool = Pool {
            state: RwLock::new(PoolState::default()),
            persist: Some(persist),
        };
        pool.load_meta()?;
        pool.replay_events()?;
        pool.load_blob_files()?;
        Ok(pool)
    }

    /// In-memory pool with no persistence. Everything is lost on drop.
    pub fn memory() -> Result<Self> {
        Ok(Pool {
            state: RwLock::new(PoolState::default()),
            persist: None,
        })
    }

    fn read(&self) -> LockResult<RwLockReadGuard<'_, PoolState>> {
        self.state.read()
    }

    fn write(&self) -> LockResult<RwLockWriteGuard<'_, PoolState>> {
        self.state.write()
    }

    // ── Persistence ────────────────────────────────────────────────────

    fn load_meta(&self) -> Result<()> {
        let Some(p) = &self.persist else { return Ok(()) };
        if !p.meta_path.exists() {
            return Ok(());
        }
        let raw = std::fs::read_to_string(&p.meta_path)
            .with_context(|| format!("failed to read {}", p.meta_path.display()))?;
        let meta: MetaState = serde_json::from_str(&raw)
            .with_context(|| format!("invalid meta JSON in {}", p.meta_path.display()))?;
        self.write().map_err(lock_err)?.meta = meta;
        Ok(())
    }

    fn save_meta(&self, meta: &MetaState) -> Result<()> {
        let Some(p) = &self.persist else { return Ok(()) };
        let tmp = p.meta_path.with_extension("meta.json.tmp");
        let json = serde_json::to_string_pretty(meta).context("meta serialization")?;
        std::fs::write(&tmp, json)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, &p.meta_path)
            .with_context(|| format!("failed to replace {}", p.meta_path.display()))?;
        Ok(())
    }

    /// Rebuild memory state from the event log. A truncated final line
    /// (crash mid-append) is discarded; corruption anywhere else is fatal.
    fn replay_events(&self) -> Result<()> {
        let Some(p) = &self.persist else { return Ok(()) };
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
                    let mut state = self.write().map_err(lock_err)?;
                    insert_new(&mut state, &env);
                    drop(state);
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

    fn load_blob_files(&self) -> Result<()> {
        let Some(p) = &self.persist else { return Ok(()) };
        let mut state = self.write().map_err(lock_err)?;
        for entry in std::fs::read_dir(&p.blobs_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            // File name: <blob_id>.<index>
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((blob_id, idx)) = name.rsplit_once('.') else { continue };
            let Ok(index) = idx.parse::<u32>() else { continue };
            let data = std::fs::read(entry.path())?;
            state.blob_pieces.insert((blob_id.to_owned(), index), data);
        }
        Ok(())
    }

    fn append_event(&self, json: &str) -> Result<()> {
        let Some(p) = &self.persist else { return Ok(()) };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p.events_path)
            .with_context(|| format!("failed to open {}", p.events_path.display()))?;
        f.write_all(json.as_bytes())?;
        f.write_all(b"\n")?;
        f.flush()?;
        Ok(())
    }

    fn write_blob_file(&self, blob_id: &str, piece_index: u32, data: &[u8]) -> Result<()> {
        let Some(p) = &self.persist else { return Ok(()) };
        let path = p.blobs_dir.join(format!("{blob_id}.{piece_index}"));
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Rewrite the event log from memory, dropping superseded and pruned events.
    pub fn compact(&self) -> Result<()> {
        let Some(p) = &self.persist else { return Ok(()) };
        let state = self.read().map_err(lock_err)?;
        let tmp = p.events_path.with_extension("jsonl.tmp");
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)
                .with_context(|| format!("failed to open {}", tmp.display()))?;
            for entry in state.events.iter().flatten() {
                let json = canonical(&entry.env)?;
                f.write_all(json.as_bytes())?;
                f.write_all(b"\n")?;
            }
            f.flush()?;
        }
        std::fs::rename(&tmp, &p.events_path)
            .with_context(|| format!("failed to replace {}", p.events_path.display()))?;
        Ok(())
    }

    // ── Event storage ──────────────────────────────────────────────────

    /// Store envelope and return outermost datum_id. Duplicates are ignored.
    pub fn put(&self, env: &Envelope) -> Result<String> {
        let json = canonical(env)?;

        {
            let mut state = self.write().map_err(lock_err)?;
            let id = datum_id(env);

            // Committed envelopes (depth 2) supersede uncommitted versions of
            // the same eid; acks (depth 3) supersede committed ones. Private
            // messages (room = None) are excluded: the ack is separate
            // information from the original, not a replacement.
            let inner = innermost(env);
            let key_room = inner.d.r.clone();
            let superseded: Vec<String> = if let Some(room) = &key_room {
                let channel = channel_of(&inner.d);
                state.events.iter()
                    .filter_map(|s| s.as_ref())
                    .filter(|e| {
                        // eid match: same room + signer + ts + channel.
                        let matches_key = e.room.as_deref() == Some(room.as_str())
                            && e.n == inner.d.n
                            && e.ts == inner.d.ts
                            && e.channel == channel;
                        let replaced = if depth(env) >= 3 {
                            e.tc.is_some()
                        } else {
                            e.tc.is_none()
                        };
                        matches_key && replaced
                    })
                    .map(|e| e.id.clone())
                    .collect()
            } else {
                Vec::new()
            };
            for sid in superseded {
                remove_id(&mut state, &sid);
            }

            if state.by_id.contains_key(&id) {
                drop(state);
            } else {
                insert_new(&mut state, env);
                drop(state);
                self.append_event(&json)?;
            }
        }

        Ok(datum_id(env))
    }

    pub fn get(&self, id: &str) -> Result<Option<Envelope>> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.by_id.get(id).and_then(|&i| state.events[i].as_ref()).map(|e| e.env.clone()))
    }

    pub fn get_room_channel(&self, room: &str, channel: i32) -> Result<Vec<PoolEntry>> {
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| e.room.as_deref() == Some(room) && e.channel == Some(channel))
            .cloned().collect();
        out.sort_by(cmp_committed_first);
        Ok(out)
    }

    /// Returns the latest commit timestamp (tc) for committed events in a room.
    pub fn latest_tc(&self, room: &str) -> Result<u64> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| e.room.as_deref() == Some(room))
            .filter_map(|e| e.tc)
            .max()
            .unwrap_or(0))
    }

    /// Room watermark: the latest commit time across all room-internal
    /// channels (`ch < 0` — metadata, membership, …).
    ///
    /// Clients compare their known value against the relay's to detect a
    /// stale view of room state (membership/metadata changes they have not
    /// seen). Advisory only — posting eligibility is enforced by the room
    /// key holder at commit time, so a forged watermark can cause at worst
    /// extra syncs or delayed refusals, never false acceptance.
    pub fn room_watermark(&self, room: &str) -> Result<u64> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| e.room.as_deref() == Some(room))
            .filter(|e| e.channel.is_some_and(|c| c < 0))
            .filter_map(|e| e.tc)
            .max()
            .unwrap_or(0))
    }

    pub fn get_room_ids(&self, room: &str) -> Result<Vec<String>> {
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<&PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| e.room.as_deref() == Some(room))
            .collect();
        out.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
        Ok(out.into_iter().map(|e| e.id.clone()).collect())
    }

    pub fn get_private(&self, node_pub: &str) -> Result<Vec<PoolEntry>> {
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| {
                matches!(e.event_type.as_str(), "private.message" | "private.read" | "private.blob")
                    && (e.n == node_pub || e.to_node.as_deref() == Some(node_pub))
            })
            .cloned().collect();
        out.sort_by(cmp_committed_first);
        Ok(out)
    }

    pub fn build_membership(&self, room: &str) -> Result<MembershipState> {
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
                    if let Some(target) = d
                        .c
                        .as_ref()
                        .and_then(|c| c.get("banned"))
                        .and_then(|v| v.as_str())
                    {
                        state.members.remove(target);
                        state.banned.insert(target.to_owned());
                    }
                }
                "room.unban" => {
                    if let Some(target) = d
                        .c
                        .as_ref()
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

        Ok(state)
    }

    pub fn get_uncommitted(&self, room: &str) -> Result<Vec<PoolEntry>> {
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| e.room.as_deref() == Some(room) && e.tc.is_none())
            .cloned().collect();
        out.sort_by(cmp_ts_id);
        Ok(out)
    }

    /// Return committed envelopes matching optional room and type filters, after `after_tc`.
    ///
    /// - `rooms`: if empty, all rooms are included.
    /// - `types`: if empty, all event types are included.
    /// - `after_tc`: only events with `tc > after_tc` (use 0 for all).
    /// - `limit`: maximum rows returned.
    pub fn get_wanted(
        &self,
        rooms: &[&str],
        types: &[&str],
        after_tc: u64,
        limit: usize,
    ) -> Result<Vec<PoolEntry>> {
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| {
                if e.tc.is_none() || e.tc.unwrap_or(0) <= after_tc {
                    return false;
                }
                (rooms.is_empty() || e.room.as_deref().map(|r| rooms.contains(&r)).unwrap_or(false))
                    && (types.is_empty() || types.contains(&e.event_type.as_str()))
            })
            .cloned().collect();
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
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| {
                let tc = match e.tc {
                    Some(tc) if tc >= start_tc => tc,
                    _ => return false,
                };
                if let Some(end) = end_tc {
                    if tc > end {
                        return false;
                    }
                }
                e.room.as_deref() == Some(room) && e.channel == Some(channel)
            })
            .cloned().collect();
        out.sort_by(cmp_tc);
        out.truncate(limit);
        Ok(out)
    }

    /// Fetch envelopes by a list of datum IDs.
    pub fn get_by_datum_ids(&self, ids: &[&str]) -> Result<Vec<PoolEntry>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| ids.contains(&e.id.as_str()))
            .cloned().collect();
        out.sort_by(cmp_tc);
        Ok(out)
    }

    /// Return per-channel stats (channel, committed_count, latest_tc) for a room.
    /// Only non-negative channels (user content) are included.
    pub fn get_room_channel_stats(&self, room: &str) -> Result<Vec<(i32, u64, u64)>> {
        let state = self.read().map_err(lock_err)?;
        let mut per_channel: HashMap<i32, (u64, u64)> = HashMap::new();
        for e in state.events.iter().filter_map(|s| s.as_ref()) {
            let Some(tc) = e.tc else { continue };
            let Some(ch) = e.channel else { continue };
            if ch < 0 || e.room.as_deref() != Some(room) {
                continue;
            }
            let slot = per_channel.entry(ch).or_insert((0, 0));
            slot.0 += 1;
            slot.1 = slot.1.max(tc);
        }
        let mut out: Vec<(i32, u64, u64)> =
            per_channel.into_iter().map(|(ch, (c, tc))| (ch, c, tc)).collect();
        out.sort_by_key(|(ch, _, _)| *ch);
        Ok(out)
    }

    /// Return distinct room public keys where `node_pub` has events.
    pub fn get_node_rooms(&self, node_pub: &str) -> Result<Vec<String>> {
        let state = self.read().map_err(lock_err)?;
        let mut rooms: HashSet<String> = HashSet::new();
        for e in state.events.iter().filter_map(|s| s.as_ref()) {
            if e.n == node_pub {
                if let Some(r) = &e.room {
                    rooms.insert(r.clone());
                }
            }
        }
        let mut out: Vec<String> = rooms.into_iter().collect();
        out.sort();
        Ok(out)
    }

    // ── TOFU Pin Storage (§4.4, §11.3.1) ──────────────────────────────

    /// Check (and optionally record) a TOFU pin for a node.
    /// `fingerprint` is the hex-encoded SHA-256 of the node's public key.
    pub fn check_tofu_pin(&self, node_pub: &str, fingerprint: &str, now: u64) -> Result<TofuResult> {
        let mut state = self.write().map_err(lock_err)?;
        match state.meta.tofu_pins.get(node_pub) {
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
                state.meta.tofu_pins.insert(
                    node_pub.to_owned(),
                    TofuPin { first_seen_at: now, fingerprint: fingerprint.to_owned() },
                );
                drop(state);
                self.save_meta(&self.read().map_err(lock_err)?.meta)?;
                Ok(TofuResult::NewPin)
            }
        }
    }

    /// Get the stored TOFU pin for a node, if any.
    pub fn get_tofu_pin(&self, node_pub: &str) -> Result<Option<(String, u64)>> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.meta.tofu_pins.get(node_pub)
            .map(|p| (p.fingerprint.clone(), p.first_seen_at)))
    }

    // ── Blob Piece Storage (§11.7) ─────────────────────────────────────

    /// Store a single blob piece.
    pub fn put_blob_piece(&self, blob_id: &str, piece_index: u32, data: &[u8]) -> Result<()> {
        self.write().map_err(lock_err)?
            .blob_pieces.insert((blob_id.to_owned(), piece_index), data.to_vec());
        self.write_blob_file(blob_id, piece_index, data)
    }

    /// Fetch a single blob piece.
    pub fn get_blob_piece(&self, blob_id: &str, piece_index: u32) -> Result<Option<Vec<u8>>> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.blob_pieces.get(&(blob_id.to_owned(), piece_index)).cloned())
    }

    /// Fetch all pieces for a blob, ordered by index. Returns None unless
    /// exactly `expected_pieces` pieces (indexed 0..N with no gaps) are stored.
    pub fn get_blob(&self, blob_id: &str, expected_pieces: u32) -> Result<Option<Vec<Vec<u8>>>> {
        let state = self.read().map_err(lock_err)?;
        let stored = state.blob_pieces.keys().filter(|(id, _)| id == blob_id).count();
        if stored != expected_pieces as usize {
            return Ok(None);
        }
        let mut out = Vec::with_capacity(expected_pieces as usize);
        for i in 0..expected_pieces {
            match state.blob_pieces.get(&(blob_id.to_owned(), i)) {
                Some(data) => out.push(data.clone()),
                None => return Ok(None),
            }
        }
        Ok(Some(out))
    }

    /// Count stored pieces for a blob.
    pub fn blob_piece_count(&self, blob_id: &str) -> Result<u32> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.blob_pieces.keys()
            .filter(|(id, _)| id == blob_id)
            .count() as u32)
    }

    // ── Local Moderation / Blocklist (§13.3) ───────────────────────────

    /// Block a node. `kind` is "block" or "spam".
    pub fn block_node(&self, node_pub: &str, kind: &str, reason: Option<&str>, now: u64) -> Result<()> {
        {
            let mut state = self.write().map_err(lock_err)?;
            state.meta.blocklist.insert(node_pub.to_owned(), BlockRecord {
                kind: kind.to_owned(),
                reason: reason.map(|r| r.to_owned()),
                blocked_at: now,
            });
        }
        self.save_meta(&self.read().map_err(lock_err)?.meta)?;
        Ok(())
    }

    /// Unblock a node.
    pub fn unblock_node(&self, node_pub: &str) -> Result<bool> {
        let removed = {
            let mut state = self.write().map_err(lock_err)?;
            state.meta.blocklist.remove(node_pub).is_some()
        };
        if removed {
            self.save_meta(&self.read().map_err(lock_err)?.meta)?;
        }
        Ok(removed)
    }

    /// Check if a node is blocked.
    pub fn is_blocked(&self, node_pub: &str) -> Result<bool> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.meta.blocklist.contains_key(node_pub))
    }

    /// Get all blocked nodes.
    pub fn get_blocklist(&self) -> Result<Vec<BlocklistEntry>> {
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<BlocklistEntry> = state.meta.blocklist.iter()
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
        let c = inner.d.c.as_ref().ok_or_else(|| anyhow!("attestation missing content"))?;
        let target = c.get("target").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("attestation missing c.target"))?;
        let level = c.get("level").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("attestation missing c.level"))?;

        {
            let mut state = self.write().map_err(lock_err)?;
            if state.meta.attestations.iter().any(|a| a.id == id) {
                drop(state);
                return Ok(id);
            }
            state.meta.attestations.push(AttestationRecord {
                id: id.clone(),
                json: canonical(env)?,
                attester: inner.d.n.clone(),
                target: target.to_owned(),
                level: level.to_owned(),
                ts: inner.d.ts,
            });
        }
        self.save_meta(&self.read().map_err(lock_err)?.meta)?;
        Ok(id)
    }

    /// Get all attestations about a target node.
    pub fn get_attestations_for(&self, target: &str) -> Result<Vec<Envelope>> {
        let state = self.read().map_err(lock_err)?;
        let mut recs: Vec<&AttestationRecord> = state.meta.attestations.iter()
            .filter(|a| a.target == target)
            .collect();
        recs.sort_by_key(|a| a.ts);
        recs.into_iter()
            .map(|a| serde_json::from_str(&a.json).context("invalid stored attestation JSON"))
            .collect()
    }

    /// Get attestation counts for a target: (positive, negative, neutral).
    pub fn attestation_counts(&self, target: &str) -> Result<(u64, u64, u64)> {
        let state = self.read().map_err(lock_err)?;
        let mut pos = 0u64;
        let mut neg = 0u64;
        let mut neu = 0u64;
        for a in &state.meta.attestations {
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
        let room = inner.d.r.as_deref()
            .ok_or_else(|| anyhow!("room.flag missing room"))?;
        let c = inner.d.c.as_ref().ok_or_else(|| anyhow!("room.flag missing content"))?;
        let target_eid = c.get("target").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("room.flag missing c.target"))?;
        let category = c.get("category").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("room.flag missing c.category"))?;

        {
            let mut state = self.write().map_err(lock_err)?;
            if state.meta.flags.iter().any(|f| f.id == id) {
                drop(state);
                return Ok(id);
            }
            state.meta.flags.push(FlagRecord {
                id: id.clone(),
                room: room.to_owned(),
                target_eid: target_eid.to_owned(),
                category: category.to_owned(),
                flagger: inner.d.n.clone(),
                ts: inner.d.ts,
            });
        }
        self.save_meta(&self.read().map_err(lock_err)?.meta)?;
        Ok(id)
    }

    /// Get flag count for a specific event in a room.
    pub fn flag_count(&self, room: &str, target_eid: &str) -> Result<u64> {
        let state = self.read().map_err(lock_err)?;
        Ok(state.meta.flags.iter()
            .filter(|f| f.room == room && f.target_eid == target_eid)
            .count() as u64)
    }

    /// Get flag counts by category for a specific event.
    pub fn flag_counts_by_category(&self, room: &str, target_eid: &str) -> Result<Vec<(String, u64)>> {
        let state = self.read().map_err(lock_err)?;
        let mut counts: HashMap<String, u64> = HashMap::new();
        for f in &state.meta.flags {
            if f.room == room && f.target_eid == target_eid {
                *counts.entry(f.category.clone()).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, u64)> = counts.into_iter().collect();
        out.sort();
        Ok(out)
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

        let removed = {
            let mut state = self.write().map_err(lock_err)?;
            let ids: Vec<String> = state.events.iter().filter_map(|s| s.as_ref())
                .filter(|e| {
                    let is_system = e.channel.is_some_and(|c| c < 0);
                    e.room.as_deref() == Some(room) && !is_system && e.ts < threshold
                })
                .map(|e| e.id.clone())
                .collect();
            for id in &ids {
                remove_id(&mut state, id);
            }
            ids.len() as u64
        };
        if removed > 0 {
            self.compact()?;
        }
        Ok(removed)
    }

    /// Prune uncommitted events older than `ttl_ms`. Returns count of removed events.
    pub fn prune_uncommitted(&self, room: &str, ttl_ms: u64) -> Result<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| anyhow!("system clock error"))?
            .as_millis() as u64;
        let threshold = now.saturating_sub(ttl_ms);

        let removed = {
            let mut state = self.write().map_err(lock_err)?;
            let ids: Vec<String> = state.events.iter().filter_map(|s| s.as_ref())
                .filter(|e| e.room.as_deref() == Some(room) && e.tc.is_none() && e.ts < threshold)
                .map(|e| e.id.clone())
                .collect();
            for id in &ids {
                remove_id(&mut state, id);
            }
            ids.len() as u64
        };
        if removed > 0 {
            self.compact()?;
        }
        Ok(removed)
    }

    // ── Export / Import (§11.10) ────────────────────────────────────────

    /// Export all envelopes for a room ordered by (ts, id).
    pub fn export_room(&self, room: &str) -> Result<Vec<Envelope>> {
        let state = self.read().map_err(lock_err)?;
        let mut out: Vec<&PoolEntry> = state.events.iter().filter_map(|s| s.as_ref())
            .filter(|e| e.room.as_deref() == Some(room))
            .collect();
        out.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
        Ok(out.into_iter().map(|e| e.env.clone()).collect())
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
fn insert_new(state: &mut PoolState, env: &Envelope) {
    let inner = innermost(env);
    let entry = PoolEntry {
        id: datum_id(env),
        env: env.clone(),
        room: inner.d.r.clone(),
        n: inner.d.n.clone(),
        to_node: inner.d.to.clone(),
        ts: inner.d.ts,
        tc: env.d.tc,
        event_type: inner.d.t.clone(),
        channel: channel_of(&inner.d),
    };
    state.events.push(Some(entry));
    state.by_id.insert(datum_id(env), state.events.len() - 1);
}

fn remove_id(state: &mut PoolState, id: &str) {
    if let Some(idx) = state.by_id.remove(id) {
        state.events[idx] = None;
    }
}

/// Committed events first (tc ASC), then uncommitted (ts ASC, id ASC).
fn cmp_committed_first(a: &PoolEntry, b: &PoolEntry) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.tc, b.tc) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.ts.cmp(&b.ts)).then_with(|| a.id.cmp(&b.id)),
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
    use std::collections::HashMap;

    use serde_json::json;

    use crate::base64url;
    use crate::crypto::{ed25519_generate, x25519_generate};
    use crate::envelope::{sign_envelope, wrap_commit};
    use crate::events::{make_room_create, make_room_update};
    use crate::types::{DatumBody, Envelope, Keypair, LocalNode, LocalRoom};

    use super::{ruleset_from_pool, Pool};

    fn mk_node() -> LocalNode {
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

    fn mk_room() -> LocalRoom {
        let (room_pub, room_seed) = ed25519_generate();
        LocalRoom {
            sig: Keypair {
                pub_key: base64url::encode(&room_pub),
                priv_key: base64url::encode(&room_seed),
            },
        }
    }

    fn mk_room_event(
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

    fn mk_private_message(sender: &LocalNode, recipient_sig: &str, ts: u64) -> Envelope {
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
        let env = mk_room_event(&node, &room, "room.message", 100, json!({"ch": 1, "body": "x"}));

        let id1 = pool.put(&env).expect("put should succeed");
        let id2 = pool.put(&env).expect("duplicate put should succeed");
        assert_eq!(id1, id2);

        let got = pool.get(&id1).expect("get should succeed").expect("must exist");
        assert_eq!(got.p, env.p);

        let entries = pool
            .get_room_channel(&room.sig.pub_key, 1)
            .expect("query should succeed");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn committed_event_replaces_uncommitted_with_same_eid() {
        let pool = Pool::memory().expect("pool should initialize");
        let author = mk_node();
        let room = mk_room();
        let inner =
            mk_room_event(&author, &room, "room.message", 101, json!({"ch": 2, "body": "hello"}));

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

        let e1 = mk_room_event(&author, &room, "room.message", 1000, json!({"ch": 1, "body": "a"}));
        let e2 = mk_room_event(&author, &room, "room.message", 2000, json!({"ch": 1, "body": "b"}));
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

        let state = pool.build_membership(&key_holder.sig.pub_key).expect("membership");
        assert!(state.members.contains(&arta.sig.pub_key));
        assert!(!state.banned.contains(&arta.sig.pub_key));
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

        let (_d1, commit) = make_room_create(&node, &room, Some("Test Room"), Some(rules))
            .expect("room.create");
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
        let (_d1, commit1) = make_room_create(&node, &room, Some("Room"), Some(rules1))
            .expect("room.create");
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
            super::TofuResult::Mismatch { expected, first_seen_at } => {
                assert_eq!(expected, "fp_original");
                assert_eq!(first_seen_at, 100);
            }
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    // ---- Blob piece storage ----

    #[test]
    fn blob_piece_store_and_fetch() {
        let pool = Pool::memory().expect("pool");
        pool.put_blob_piece("blob1", 0, b"piece0").unwrap();
        pool.put_blob_piece("blob1", 1, b"piece1").unwrap();

        assert_eq!(pool.blob_piece_count("blob1").unwrap(), 2);
        assert_eq!(pool.get_blob_piece("blob1", 0).unwrap().unwrap(), b"piece0");
        assert_eq!(pool.get_blob_piece("blob1", 1).unwrap().unwrap(), b"piece1");
        assert!(pool.get_blob_piece("blob1", 2).unwrap().is_none());
    }

    #[test]
    fn get_blob_returns_all_pieces_in_order() {
        let pool = Pool::memory().expect("pool");
        pool.put_blob_piece("blob2", 0, b"aaa").unwrap();
        pool.put_blob_piece("blob2", 1, b"bbb").unwrap();
        pool.put_blob_piece("blob2", 2, b"ccc").unwrap();

        let pieces = pool.get_blob("blob2", 3).unwrap().unwrap();
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0], b"aaa");
        assert_eq!(pieces[2], b"ccc");

        // Wrong expected count returns None
        assert!(pool.get_blob("blob2", 2).unwrap().is_none());
    }

    // ---- Local moderation ----

    #[test]
    fn block_and_unblock_node() {
        let pool = Pool::memory().expect("pool");
        assert!(!pool.is_blocked("NODE_X").unwrap());

        pool.block_node("NODE_X", "block", Some("spam"), 1000).unwrap();
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
            &attester, &target.sig.pub_key, "positive", Some("good peer"),
        ).unwrap();
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
            &node, &room.sig.pub_key, "eid_123", "spam", Some("offensive"),
        ).unwrap();
        pool.put_flag(&flag).unwrap();

        assert_eq!(pool.flag_count(&room.sig.pub_key, "eid_123").unwrap(), 1);

        let by_cat = pool.flag_counts_by_category(&room.sig.pub_key, "eid_123").unwrap();
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
        let meta = mk_room_event(&node, &room, "room.update", 2,
            json!({"name": "Old Name", "rules": []}));
        pool.put(&wrap_commit(join, &room, 10).unwrap()).unwrap();
        pool.put(&wrap_commit(meta, &room, 11).unwrap()).unwrap();

        // Ancient content event.
        let msg = mk_room_event(&node, &room, "room.message", 3, json!({"ch": 1, "body": "old"}));
        pool.put(&wrap_commit(msg, &room, 12).unwrap()).unwrap();

        // Retention window that expires everything.
        let pruned = pool.prune_retention(&room.sig.pub_key, 1).unwrap();
        assert_eq!(pruned, 1, "only the content event is prunable");

        let state = pool.build_membership(&room.sig.pub_key).unwrap();
        assert!(state.members.contains(&node.sig.pub_key),
            "membership must survive retention pruning");
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

        assert_eq!(pool.room_watermark(&room.sig.pub_key).unwrap(), 0,
            "empty room has watermark 0");

        let join = mk_room_event(&node, &room, "room.join", 100, json!({}));
        pool.put(&wrap_commit(join, &room, 500).unwrap()).unwrap();
        assert_eq!(pool.room_watermark(&room.sig.pub_key).unwrap(), 500);

        // Newer metadata commit advances the watermark.
        let meta = mk_room_event(&node, &room, "room.update", 900,
            json!({"name": "Renamed", "rules": []}));
        pool.put(&wrap_commit(meta, &room, 1000).unwrap()).unwrap();
        assert_eq!(pool.room_watermark(&room.sig.pub_key).unwrap(), 1000);

        // Content commits do not advance the room watermark.
        let msg = mk_room_event(&node, &room, "room.message", 2000, json!({"ch": 1, "body": "x"}));
        pool.put(&wrap_commit(msg, &room, 5000).unwrap()).unwrap();
        assert_eq!(pool.room_watermark(&room.sig.pub_key).unwrap(), 1000,
            "content channel commits must not advance the watermark");

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
            let pool = Pool::open(&path).expect("open");
            let node = mk_node();
            let room = mk_room();
            room_key = room.sig.pub_key.clone();

            let e1 = mk_room_event(&node, &room, "room.message", 100, json!({"ch": 1, "body": "a"}));
            let c1 = wrap_commit(e1, &room, 200).expect("commit");
            pool.put(&c1).expect("put");
            pool.check_tofu_pin("NODE_P", "fp", 5).expect("pin");
            pool.block_node("NODE_SPAM", "block", None, 6).expect("block");
            pool.put_blob_piece("blobz", 0, b"zzz").expect("piece");
        }

        let pool = Pool::open(&path).expect("reopen");
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
            let pool = Pool::open(&path).expect("open");
            let node = mk_node();
            let room = mk_room();
            let env = mk_room_event(&node, &room, "room.message", 42, json!({"ch": 1, "body": "ok"}));
            pool.put(&env).expect("put");
        }

        // Simulate a crash mid-append: append a partial JSON line.
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&events_path).unwrap();
            f.write_all(b"{\"d\":{\"n\":\"BRO").unwrap();
        }

        Pool::open(&path).expect("reopen with truncated tail");
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
            let pool = Pool::open(&path).expect("open");
            let node = mk_node();
            let room = mk_room();
            room_key = room.sig.pub_key.clone();

            // Two old messages (ts ~ 0) and one fresh one.
            let old1 = mk_room_event(&node, &room, "room.message", 1, json!({"ch": 1, "body": "old1"}));
            let old2 = mk_room_event(&node, &room, "room.message", 2, json!({"ch": 1, "body": "old2"}));
            let fresh = mk_room_event(&node, &room, "room.message", u64::MAX / 2, json!({"ch": 1, "body": "new"}));
            for e in [old1, old2, fresh] {
                let ts = e.d.ts;
                pool.put(&wrap_commit(e, &room, ts).unwrap()).unwrap();
            }
        }

        let pool = Pool::open(&path).expect("reopen");
        let pruned = pool.prune_retention(&room_key, 1).expect("prune");
        assert_eq!(pruned, 2);

        drop(pool);
        let pool = Pool::open(&path).expect("reopen after prune");
        assert_eq!(pool.get_room_channel(&room_key, 1).unwrap().len(), 1);

        std::fs::remove_dir_all(&base).ok();
    }
}
