import { createMemo, createSignal } from "solid-js";
import * as db from "./db";
import type { EnvelopeRecord, RelayRecord, RoomRecord } from "./db";
import {
    RelayConnection,
    parseEndpoint,
    type RelayEndpoint,
} from "./transport";
import { downloadText } from "./ui";
import * as protocol from "./wasm";
import type { Envelope, Identity, RoomIdentity } from "./wasm";

export type SyncState = "idle" | "syncing" | "offline" | "error";

export interface Message {
    eid: string;
    author: string;
    body: string;
    ts: number;
    depth: number;
    tc: number | null;
    envelopeJson: string;
    mine: boolean;
}

export interface RoomSummary {
    preview: string;
    ts: number;
    unread: number;
}

const CHAT_CHANNEL = 1;
const DEFAULT_RULES = [
    { t: "join", allow: "*" },
    { t: "post", allow: "*" },
];

function randomId(): string {
    if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
        return crypto.randomUUID();
    }
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function errorText(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

function shortRoomName(pub: string): string {
    return `Room ${pub.slice(0, 6)}`;
}

function previewOf(body: string): string {
    const text = body.replace(/\s+/g, " ").trim();
    return text.length > 44 ? `${text.slice(0, 44)}…` : text;
}

export function createTolooClient() {
    const [initialized, setInitialized] = createSignal(false);
    const [identity, setIdentity] = createSignal<Identity | null>(null);
    const [relays, setRelays] = createSignal<RelayRecord[]>([]);
    const [rooms, setRooms] = createSignal<RoomRecord[]>([]);
    const [roomSummaries, setRoomSummaries] = createSignal<
        Record<string, RoomSummary>
    >({});
    const [activePub, setActivePub] = createSignal<string | null>(null);
    const [messages, setMessages] = createSignal<Message[]>([]);
    const [members, setMembers] = createSignal<string[]>([]);
    const [banned, setBanned] = createSignal<string[]>([]);
    const [syncState, setSyncState] = createSignal<SyncState>("idle");
    const [syncing, setSyncing] = createSignal(false);
    const [lastSync, setLastSync] = createSignal<number | null>(null);
    const [blocked, setBlocked] = createSignal<string[]>([]);
    const [feedback, setFeedback] = createSignal("");

    const activeRoom = createMemo(
        () => rooms().find((room) => room.pub === activePub()) ?? null,
    );

    const connections = new Map<string, RelayConnection>();

    function notify(
        text: string,
        kind: "info" | "success" | "error" = "info",
    ): void {
        if (kind === "error") setFeedback(text);
    }

    function dismissFeedback(): void {
        setFeedback("");
    }

    function connectionFor(relay: RelayRecord): RelayConnection {
        const existing = connections.get(relay.id);
        if (existing) return existing;
        const endpoint: RelayEndpoint = { proto: relay.proto, url: relay.url };
        const connection = new RelayConnection(endpoint);
        connections.set(relay.id, connection);
        return connection;
    }

    function closeConnection(id: string): void {
        connections.get(id)?.close();
        connections.delete(id);
    }

    async function refreshSummary(roomPub: string): Promise<void> {
        const records = await db.roomEnvelopes(roomPub);
        const mark = rooms().find((room) => room.pub === roomPub)?.lastReadAt ?? 0;
        const self = identity()?.sigPub;
        const unread = new Set<string>();
        let latest: EnvelopeRecord | null = null;
        for (const record of records) {
            if (record.type !== "room.message" || !record.body) continue;
            if (blocked().includes(record.signer)) continue;
            if (!latest || record.ts > latest.ts) latest = record;
            if (record.ts > mark && record.signer !== self) {
                unread.add(record.eid);
            }
        }
        setRoomSummaries((current) => ({
            ...current,
            [roomPub]: latest
                ? {
                      preview: previewOf(latest.body ?? ""),
                      ts: latest.ts,
                      unread: unread.size,
                  }
                : { preview: "", ts: 0, unread: 0 },
        }));
    }

    async function load(): Promise<void> {
        const [savedIdentity, savedRelays, savedRooms, savedBlocklist, active] =
            await Promise.all([
                db.getIdentity(),
                db.listRelays(),
                db.listRooms(),
                db.getSetting<string[]>("blocklist"),
                db.getSetting<string>("activeRoom"),
            ]);
        setIdentity(savedIdentity ?? null);
        setRelays(savedRelays);
        setRooms(savedRooms);
        setBlocked(savedBlocklist ?? []);
        await Promise.all(savedRooms.map((room) => refreshSummary(room.pub)));
        if (active && savedRooms.some((room) => room.pub === active)) {
            setActivePub(active);
            await refreshRoom(active);
        }
        setInitialized(true);
    }

    async function createIdentity(): Promise<Identity> {
        const generated = protocol.generateIdentity();
        await db.saveIdentity(generated);
        setIdentity(generated);
        return generated;
    }

    async function restoreIdentity(
        sigSeed: string,
        encPriv: string,
    ): Promise<Identity> {
        const restored = protocol.importIdentity(sigSeed.trim(), encPriv.trim());
        await db.saveIdentity(restored);
        setIdentity(restored);
        return restored;
    }

    async function restoreIdentityJson(raw: string): Promise<Identity> {
        let value: Record<string, unknown>;
        try {
            value = JSON.parse(raw) as Record<string, unknown>;
        } catch {
            throw new Error("Identity file is not valid JSON.");
        }
        const sig = value.sig as Record<string, unknown> | undefined;
        const enc = value.enc as Record<string, unknown> | undefined;
        const sigSeed =
            (value.sigSeed as string | undefined) ??
            (sig?.priv_key as string | undefined) ??
            (value.sig_priv as string | undefined);
        const encPriv =
            (value.encPriv as string | undefined) ??
            (enc?.priv_key as string | undefined) ??
            (value.enc_priv as string | undefined);
        if (!sigSeed || !encPriv) {
            throw new Error("Identity file is missing private keys.");
        }
        return restoreIdentity(sigSeed, encPriv);
    }

    async function addRelay(raw: string): Promise<RelayRecord> {
        const endpoint = parseEndpoint(raw);
        if (relays().some((relay) => relay.url === endpoint.url)) {
            throw new Error("That relay is already configured.");
        }
        const record: RelayRecord = {
            id: randomId(),
            url: endpoint.url,
            proto: endpoint.proto,
            createdAt: Date.now(),
        };
        await db.saveRelay(record);
        setRelays((current) => [...current, record]);
        notify("Relay added.", "success");
        return record;
    }

    async function removeRelay(id: string): Promise<void> {
        closeConnection(id);
        await db.deleteRelay(id);
        setRelays((current) => current.filter((relay) => relay.id !== id));
    }

    async function updateRoomRecord(room: RoomRecord): Promise<void> {
        await db.saveRoom(room);
        setRooms((current) =>
            current.map((item) => (item.pub === room.pub ? room : item)),
        );
    }

    async function storeEnvelopeJson(envelopeJson: string): Promise<EnvelopeRecord> {
        const envelope = JSON.parse(envelopeJson) as Envelope;
        const info = protocol.info(envelope);
        const body =
            typeof info.content?.body === "string" ? info.content.body : null;
        const record: EnvelopeRecord = {
            datumId: info.datumId,
            eid: info.eid,
            envelope: JSON.stringify(envelope),
            room: info.room,
            to: info.to,
            type: info.type,
            channel: info.channel,
            ts: info.ts,
            tc: info.tc,
            depth: info.depth,
            signer: info.signer,
            body,
        };
        await db.putEnvelopes([record]);
        return record;
    }

    async function storeVerifiedResponses(raw: string[]): Promise<number> {
        if (raw.length === 0) return 0;
        const envelopes: Envelope[] = [];
        for (const item of raw) {
            try {
                const parsed = JSON.parse(item) as unknown;
                if (
                    typeof parsed === "object" &&
                    parsed !== null &&
                    !Array.isArray(parsed) &&
                    "d" in parsed &&
                    "p" in parsed
                ) {
                    envelopes.push(parsed as Envelope);
                }
            } catch {
                // Ignore malformed relay frames.
            }
        }
        if (envelopes.length === 0) return 0;

        const verdicts = protocol.verifyEnvelopes(envelopes);
        const valid = envelopes.filter((_, index) => verdicts[index]?.ok);
        if (valid.length === 0) return 0;

        const records: EnvelopeRecord[] = [];
        for (const envelope of valid) {
            const info = protocol.info(envelope);
            records.push({
                datumId: info.datumId,
                eid: info.eid,
                envelope: JSON.stringify(envelope),
                room: info.room,
                to: info.to,
                type: info.type,
                channel: info.channel,
                ts: info.ts,
                tc: info.tc,
                depth: info.depth,
                signer: info.signer,
                body:
                    typeof info.content?.body === "string"
                        ? info.content.body
                        : null,
            });
        }
        await db.putEnvelopes(records);
        return records.length;
    }

    async function requestFromRelays(
        offers: Envelope[],
        wantRooms: string[],
        after = 0,
        limit = 500,
    ): Promise<number> {
        const currentIdentity = identity();
        if (!currentIdentity) throw new Error("Create an identity first.");
        const configured = relays();
        if (configured.length === 0) throw new Error("Add a relay first.");

        const request = protocol.poolExchangeRequest(currentIdentity, {
            offer: offers,
            want: { rooms: wantRooms, after, limit },
        });
        const results = await Promise.allSettled(
            configured.map((relay) => connectionFor(relay).request(request)),
        );
        const raw: string[] = [];
        const errors: string[] = [];
        for (const result of results) {
            if (result.status === "fulfilled") {
                raw.push(...result.value);
            } else {
                errors.push(errorText(result.reason));
            }
        }
        if (raw.length === 0 && errors.length === configured.length) {
            throw new Error(errors[0] ?? "All relays are unreachable.");
        }
        return storeVerifiedResponses(raw);
    }

    async function refreshRoom(roomPub: string): Promise<void> {
        const records = await db.roomEnvelopes(roomPub);
        const currentIdentity = identity();

        const deleted = new Set<string>();
        for (const record of records) {
            if (record.type !== "room.delete") continue;
            try {
                const content = protocol.info(record.envelope).content;
                const target = content?.target;
                if (typeof target === "string") deleted.add(target);
            } catch {
                // Ignore malformed deletion records.
            }
        }
        const best = new Map<string, EnvelopeRecord>();
        for (const record of records) {
            if (record.type !== "room.message") continue;
            if (!record.body) continue;
            if (deleted.has(record.eid)) continue;
            if (blocked().includes(record.signer)) continue;
            const previous = best.get(record.eid);
            if (!previous || record.depth > previous.depth) {
                best.set(record.eid, record);
            }
        }
        const list: Message[] = [...best.values()]
            .map((record) => ({
                eid: record.eid,
                author: record.signer,
                body: record.body ?? "",
                ts: record.ts,
                depth: record.depth,
                tc: record.tc,
                envelopeJson: record.envelope,
                mine: record.signer === currentIdentity?.sigPub,
            }))
            .sort((a, b) => a.ts - b.ts || a.eid.localeCompare(b.eid));
        const previous = messages();
        if (
            document.hidden &&
            previous.length > 0 &&
            "Notification" in window &&
            Notification.permission === "granted" &&
            localStorage.getItem("toloo-notifications") !== "0"
        ) {
            const known = new Set(previous.map((message) => message.eid));
            const roomName =
                rooms().find((room) => room.pub === roomPub)?.name ?? "Toloo";
            for (const message of list) {
                if (known.has(message.eid) || message.mine) continue;
                new Notification(`Toloo · ${roomName}`, {
                    body: message.body.slice(0, 140),
                });
            }
        }
        setMessages(list);
        const latestMessage = list.at(-1);
        const roomRecord = rooms().find((room) => room.pub === roomPub);
        if (
            activePub() === roomPub &&
            roomRecord &&
            latestMessage &&
            latestMessage.ts > (roomRecord.lastReadAt ?? 0)
        ) {
            await updateRoomRecord({
                ...roomRecord!,
                lastReadAt: latestMessage.ts,
            });
        }
        await refreshSummary(roomPub);

        const membershipEvents = records
            .filter(
                (record) =>
                    record.tc !== null &&
                    record.channel === -2 &&
                    ["room.join", "room.leave", "room.ban", "room.unban"].includes(
                        record.type,
                    ),
            )
            .map((record) => JSON.parse(record.envelope) as Envelope);
        try {
            const state = protocol.membership(roomPub, membershipEvents);
            setMembers(state.members);
            setBanned(state.banned);
        } catch {
            setMembers([]);
            setBanned([]);
        }

        const metadata = records
            .filter(
                (record) =>
                    record.channel === -1 &&
                    record.tc !== null &&
                    (record.type === "room.create" || record.type === "room.update"),
            )
            .sort((a, b) => (a.tc ?? 0) - (b.tc ?? 0));
        const latest = metadata.at(-1);
        if (latest) {
            const envelope = JSON.parse(latest.envelope) as Envelope;
            const content = protocol.info(envelope).content;
            const name = typeof content?.name === "string" ? content.name : null;
            const room = rooms().find((item) => item.pub === roomPub);
            if (name && room && room.name !== name) {
                await updateRoomRecord({ ...room, name });
            }
        }
    }

    async function selectRoom(roomPub: string): Promise<void> {
        setActivePub(roomPub);
        await db.saveSetting("activeRoom", roomPub);
        setMessages([]);
        setMembers([]);
        setBanned([]);
        await refreshRoom(roomPub);
    }

    /**
     * The browser holding a room key acts as the room authority while online:
     * wrap uncommitted room events in commits and publish them.
     */
    async function commitPending(room: RoomRecord): Promise<number> {
        if (!room.roomIdentity) return 0;
        const records = await db.roomEnvelopes(room.pub);
        const committedEids = new Set(
            records
                .filter((record) => record.depth >= 2)
                .map((record) => record.eid),
        );
        const pending = records.filter(
            (record) =>
                record.depth === 1 &&
                record.room === room.pub &&
                !committedEids.has(record.eid) &&
                record.type !== "room.create" &&
                ["room.message", "room.join", "room.leave", "room.invite"].includes(
                    record.type,
                ),
        );
        if (pending.length === 0) return 0;

        let maxTc = records.reduce(
            (highest, record) => Math.max(highest, record.tc ?? 0),
            0,
        );
        const commits: Envelope[] = [];
        for (const record of pending) {
            const envelope = JSON.parse(record.envelope) as Envelope;
            const tc = Math.max(Date.now(), record.ts, maxTc + 1);
            maxTc = tc;
            const commit = protocol.commitEnvelope(
                envelope,
                room.roomIdentity,
                tc,
            );
            await storeEnvelopeJson(JSON.stringify(commit));
            commits.push(commit);
        }

        if (relays().length > 0) {
            await requestFromRelays(commits, [room.pub], 0, 200);
        }
        return commits.length;
    }

    async function syncRoom(roomPub = activePub(), quiet = false): Promise<void> {
        if (!roomPub || !identity()) return;
        if (relays().length === 0) {
            setSyncState("offline");
            return;
        }
        if (syncing()) return;
        setSyncing(true);
        setSyncState("syncing");
        try {
            await requestFromRelays([], [roomPub], 0, 500);
            await refreshRoom(roomPub);
            const room = rooms().find((item) => item.pub === roomPub);
            if (room?.roomIdentity) {
                const committed = await commitPending(room);
                if (committed > 0) await refreshRoom(roomPub);
            }
            setLastSync(Date.now());
            setSyncState("idle");
        } catch (error) {
            setSyncState("error");
            if (!quiet) notify(errorText(error), "error");
        } finally {
            setSyncing(false);
        }
    }

    async function createRoom(nameInput: string): Promise<RoomRecord> {
        const currentIdentity = identity();
        if (!currentIdentity) throw new Error("Create an identity first.");
        const name = nameInput.trim() || "Untitled room";
        const roomIdentity: RoomIdentity = protocol.generateRoomIdentity();
        const pair = protocol.roomCreate(
            currentIdentity,
            roomIdentity,
            name,
            DEFAULT_RULES,
        );
        const committedJson = JSON.stringify(pair.committed);
        const room: RoomRecord = {
            pub: roomIdentity.sigPub,
            name,
            createdAt: Date.now(),
            inviteUri: protocol.encodeInvite([pair.committed]),
            roomIdentity,
            roomCreate: committedJson,
        };
        await db.saveRoom(room);
        setRooms((current) => [...current, room]);
        await storeEnvelopeJson(committedJson);
        setActivePub(room.pub);
        await db.saveSetting("activeRoom", room.pub);
        await refreshRoom(room.pub);

        if (relays().length > 0) {
            try {
                await requestFromRelays([pair.committed], [], 0, 0);
                notify("Room created and published.", "success");
            } catch (error) {
                notify(
                    `Room saved locally; relay publish failed: ${errorText(error)}`,
                    "error",
                );
            }
        } else {
            notify("Room created locally. Add a relay to publish it.", "info");
        }
        return room;
    }

    async function joinRoom(input: string): Promise<RoomRecord> {
        const currentIdentity = identity();
        if (!currentIdentity) throw new Error("Create an identity first.");
        const text = input.trim();
        if (!text) throw new Error("Paste an invite or room public key.");

        let roomPub = "";
        let name = "";
        let roomCreate: string | undefined;

        if (text.startsWith("toloo:") || text.includes("\n")) {
            const envelopes = protocol.decodeInvite(text);
            for (const envelope of envelopes) {
                const info = protocol.info(envelope);
                if (info.type === "room.create" && info.room) {
                    roomPub = info.room;
                    const content = info.content;
                    name =
                        typeof content?.name === "string" ? content.name : "";
                    roomCreate = JSON.stringify(envelope);
                    break;
                }
            }
            if (!roomPub) {
                throw new Error("That invite does not contain a room.create.");
            }
        } else {
            if (!/^[A-Za-z0-9_-]{43}$/.test(text)) {
                throw new Error("A room public key must be 43 base64url characters.");
            }
            roomPub = text;
        }

        const existing = rooms().find((room) => room.pub === roomPub);
        let room: RoomRecord;
        if (existing) {
            room = existing;
        } else {
            room = {
                pub: roomPub,
                name: name || shortRoomName(roomPub),
                createdAt: Date.now(),
                roomCreate,
            };
            await db.saveRoom(room);
            setRooms((current) => [...current, room]);
        }

        const join = protocol.roomJoin(currentIdentity, roomPub);
        await storeEnvelopeJson(JSON.stringify(join));
        setActivePub(roomPub);
        await db.saveSetting("activeRoom", roomPub);
        await refreshRoom(roomPub);

        if (relays().length > 0) {
            const offers = roomCreate
                ? [(JSON.parse(roomCreate) as Envelope), join]
                : [join];
            try {
                await requestFromRelays(offers, [roomPub], 0, 500);
                await refreshRoom(roomPub);
                notify("Join request sent.", "success");
            } catch (error) {
                notify(`Join saved locally: ${errorText(error)}`, "error");
            }
        } else {
            notify("Join saved locally. Add a relay to send it.", "info");
        }
        return room;
    }

    async function importDiscovery(raw: string): Promise<RoomRecord | null> {
        const text = raw.trim();
        if (!text) throw new Error("Paste a toloo:// link or .toloo file.");
        const envelopes = protocol.decodeInvite(text);
        const knownRooms = new Set(rooms().map((room) => room.pub));
        const knownRelays = new Set(relays().map((relay) => relay.url));
        const additions: RoomRecord[] = [];
        const newRelays: RelayRecord[] = [];
        for (const envelope of envelopes) {
            const info = protocol.info(envelope);
            await storeEnvelopeJson(JSON.stringify(envelope));
            if (
                info.type === "room.create" &&
                info.room &&
                !knownRooms.has(info.room)
            ) {
                const content = info.content;
                const name =
                    typeof content?.name === "string" && content.name.trim()
                        ? content.name
                        : shortRoomName(info.room);
                knownRooms.add(info.room);
                additions.push({
                    pub: info.room,
                    name,
                    createdAt: Date.now(),
                    inviteUri: protocol.encodeInvite([envelope]),
                    roomCreate: JSON.stringify(envelope),
                });
            }
            const endpoints = Array.isArray(info.content?.endpoints)
                ? (info.content?.endpoints as Array<Record<string, unknown>>)
                : [];
            for (const endpoint of endpoints) {
                const proto = typeof endpoint.proto === "string" ? endpoint.proto : "";
                const host = typeof endpoint.host === "string" ? endpoint.host : "";
                if (!proto || !host) continue;
                const port = typeof endpoint.port === "number" ? endpoint.port : null;
                const path =
                    typeof endpoint.path === "string" && endpoint.path !== "/"
                        ? endpoint.path
                        : "";
                const url = `${proto}://${host}${port ? `:${port}` : ""}${path}`;
                if (knownRelays.has(url)) continue;
                try {
                    const parsed = parseEndpoint(url);
                    knownRelays.add(parsed.url);
                    newRelays.push({
                        id: randomId(),
                        url: parsed.url,
                        proto: parsed.proto,
                        createdAt: Date.now(),
                    });
                } catch {
                    // Endpoint protocols unsupported by this browser are skipped.
                }
            }
        }
        for (const room of additions) await db.saveRoom(room);
        for (const relay of newRelays) await db.saveRelay(relay);
        if (additions.length > 0) setRooms((current) => [...current, ...additions]);
        if (newRelays.length > 0) setRelays((current) => [...current, ...newRelays]);
        const first = additions[0];
        if (first) {
            setActivePub(first.pub);
            await db.saveSetting("activeRoom", first.pub);
            await refreshRoom(first.pub);
        }
        notify(
            `Imported ${envelopes.length} envelope(s), ${additions.length} room(s), ${newRelays.length} relay(s).`,
            "success",
        );
        return first ?? null;
    }

    async function sendMessage(bodyInput: string): Promise<void> {
        const currentIdentity = identity();
        const room = activeRoom();
        const body = bodyInput.trim();
        if (!currentIdentity) throw new Error("Create an identity first.");
        if (!room) throw new Error("Select a room first.");
        if (!body) return;

        const draft = protocol.roomMessage(
            currentIdentity,
            room.pub,
            CHAT_CHANNEL,
            body,
        );
        let outgoing = draft;
        if (room.roomIdentity) {
            const info = protocol.info(draft);
            outgoing = protocol.commitEnvelope(
                draft,
                room.roomIdentity,
                Math.max(Date.now(), info.ts),
            );
        }
        await storeEnvelopeJson(JSON.stringify(outgoing));
        await refreshRoom(room.pub);

        if (relays().length === 0) {
            notify("Message saved locally: No relay configured", "info");
            return;
        }
        const offers = room.roomCreate
            ? [(JSON.parse(room.roomCreate) as Envelope), outgoing]
            : [outgoing];
        try {
            await requestFromRelays(offers, [room.pub], 0, 200);
            await refreshRoom(room.pub);
        } catch (error) {
            notify(`Could not reach a relay: ${errorText(error)}`, "error");
        }
    }

    async function renameRoom(
        nameInput: string,
        roomPub = activePub(),
    ): Promise<void> {
        const currentIdentity = identity();
        const room = rooms().find((item) => item.pub === roomPub);
        const name = nameInput.trim();
        if (!currentIdentity) throw new Error("Create an identity first.");
        if (!room) throw new Error("Select a room first.");
        if (!name) throw new Error("Room name cannot be empty.");
        if (!room.roomIdentity) {
            throw new Error("Only the room owner can rename this room.");
        }
        const update = protocol.roomUpdate(currentIdentity, room.roomIdentity, name);
        const info = protocol.info(update);
        const outgoing = protocol.commitEnvelope(
            update,
            room.roomIdentity,
            Math.max(Date.now(), info.ts),
        );
        await storeEnvelopeJson(JSON.stringify(outgoing));
        await updateRoomRecord({ ...room, name });
        await publishEnvelopes(room, [outgoing]);
        await refreshRoom(room.pub);
        notify("Room renamed.", "success");
    }

    async function leaveRoom(roomPub = activePub()): Promise<void> {
        const currentIdentity = identity();
        const room = rooms().find((item) => item.pub === roomPub);
        if (!currentIdentity) throw new Error("Create an identity first.");
        if (!room) throw new Error("Select a room first.");
        const leave = protocol.roomLeave(currentIdentity, room.pub);
        const outgoing = room.roomIdentity
            ? protocol.commitEnvelope(
                  leave,
                  room.roomIdentity,
                  Math.max(Date.now(), protocol.info(leave).ts),
              )
            : leave;
        await storeEnvelopeJson(JSON.stringify(outgoing));
        await publishEnvelopes(room, [outgoing]);
        await removeRoom(room.pub);
        notify("Left the room.", "success");
    }

    async function exportRoomMessages(roomPub = activePub()): Promise<void> {
        if (!roomPub) throw new Error("Select a room first.");
        const records = await db.roomEnvelopes(roomPub);
        if (records.length === 0) throw new Error("No messages to export yet.");
        const envelopes = records.map(
            (record) => JSON.parse(record.envelope) as Envelope,
        );
        const text = protocol.encodeFile(envelopes);
        downloadText(text, `toloo-${roomPub.slice(0, 8)}.toloo`, "text/plain");
        notify(`Exported ${records.length} envelope(s).`, "success");
    }

    async function deleteMessage(targetEid: string): Promise<void> {
        const currentIdentity = identity();
        const room = activeRoom();
        if (!currentIdentity) throw new Error("Create an identity first.");
        if (!room) throw new Error("Select a room first.");
        const deletion = protocol.roomDelete(
            currentIdentity,
            room.pub,
            CHAT_CHANNEL,
            targetEid,
        );
        const outgoing = room.roomIdentity
            ? protocol.commitEnvelope(
                  deletion,
                  room.roomIdentity,
                  Math.max(Date.now(), protocol.info(deletion).ts),
              )
            : deletion;
        await storeEnvelopeJson(JSON.stringify(outgoing));
        await publishEnvelopes(room, [outgoing]);
        await refreshRoom(room.pub);
        notify("Message deleted.", "success");
    }

    async function flagMessage(
        targetEid: string,
        category = "spam",
    ): Promise<void> {
        const currentIdentity = identity();
        const room = activeRoom();
        if (!currentIdentity) throw new Error("Create an identity first.");
        if (!room) throw new Error("Select a room first.");
        const flag = protocol.roomFlag(
            currentIdentity,
            room.pub,
            targetEid,
            category,
        );
        const outgoing = room.roomIdentity
            ? protocol.commitEnvelope(
                  flag,
                  room.roomIdentity,
                  Math.max(Date.now(), protocol.info(flag).ts),
              )
            : flag;
        await storeEnvelopeJson(JSON.stringify(outgoing));
        await publishEnvelopes(room, [outgoing]);
        notify(`Message flagged as ${category}.`, "success");
    }

    async function attestAuthor(
        nodePub: string,
        level: "positive" | "negative",
    ): Promise<void> {
        const currentIdentity = identity();
        if (!currentIdentity) throw new Error("Create an identity first.");
        const attestation = protocol.sideAttestation(
            currentIdentity,
            nodePub,
            level,
        );
        await storeEnvelopeJson(JSON.stringify(attestation));
        if (relays().length > 0) {
            try {
                await requestFromRelays([attestation], [], 0, 200);
            } catch (error) {
                notify(
                    `Attestation saved locally: ${errorText(error)}`,
                    "error",
                );
                return;
            }
        }
        notify(`Attestation recorded: ${level}`, "success");
    }

    async function blockAuthor(nodePub: string): Promise<void> {
        if (blocked().includes(nodePub)) return;
        const next = [...blocked(), nodePub];
        await db.saveSetting("blocklist", next);
        setBlocked(next);
        if (activePub()) await refreshRoom(activePub() as string);
        notify("Author blocked on this device.", "success");
    }

    async function unblockAuthor(nodePub: string): Promise<void> {
        const next = blocked().filter((item) => item !== nodePub);
        await db.saveSetting("blocklist", next);
        setBlocked(next);
        if (activePub()) await refreshRoom(activePub() as string);
        notify("Author unblocked.", "success");
    }

    async function publishEnvelopes(
        room: RoomRecord,
        envelopes: Envelope[],
    ): Promise<void> {
        if (relays().length === 0) return;
        const offers = room.roomCreate
            ? [(JSON.parse(room.roomCreate) as Envelope), ...envelopes]
            : envelopes;
        try {
            await requestFromRelays(offers, [room.pub], 0, 200);
        } catch (error) {
            notify(`Saved locally; relay publish failed: ${errorText(error)}`, "error");
        }
    }

    async function removeRoom(roomPub: string): Promise<void> {
        await db.deleteRoom(roomPub);
        setRoomSummaries((current) => {
            const next = { ...current };
            delete next[roomPub];
            return next;
        });
        setRooms((current) => {
            const remaining = current.filter((room) => room.pub !== roomPub);
            if (activePub() === roomPub) {
                const next = remaining[0]?.pub ?? null;
                setActivePub(next);
                if (next) {
                    void db.saveSetting("activeRoom", next);
                    void refreshRoom(next);
                } else {
                    void db.saveSetting("activeRoom", "");
                    setMessages([]);
                    setMembers([]);
                }
            }
            return remaining;
        });
    }

    function exportIdentity(): string {
        const current = identity();
        if (!current) throw new Error("No identity loaded.");
        return JSON.stringify(current, null, 2);
    }

    async function removeIdentity(): Promise<void> {
        await db.deleteIdentity();
        setIdentity(null);
        notify("Identity removed from this device.", "info");
    }

    function startAutoSync(intervalMs = 12_000): () => void {
        const timer = window.setInterval(() => {
            if (!syncing() && activePub() && relays().length > 0) {
                void syncRoom(activePub() ?? undefined, true);
            }
        }, intervalMs);
        return () => window.clearInterval(timer);
    }

    return {
        initialized,
        identity,
        relays,
        rooms,
        roomSummaries,
        activePub,
        activeRoom,
        messages,
        members,
        banned,
        blocked,
        feedback,
        dismissFeedback,
        blockAuthor,
        attestAuthor,
        unblockAuthor,
        syncState,
        syncing,
        lastSync,
        load,
        createIdentity,
        restoreIdentity,
        restoreIdentityJson,
        addRelay,
        removeRelay,
        createRoom,
        joinRoom,
        importDiscovery,
        selectRoom,
        syncRoom,
        sendMessage,
        renameRoom,
        leaveRoom,
        exportRoomMessages,
        deleteMessage,
        flagMessage,
        removeRoom,
        exportIdentity,
        removeIdentity,
        notify,
        startAutoSync,
    };
}

export type TolooClient = ReturnType<typeof createTolooClient>;
