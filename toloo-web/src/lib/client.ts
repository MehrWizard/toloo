import { createMemo, createSignal } from "solid-js";
import * as db from "./db";
import type { EnvelopeRecord, RelayRecord, RoomRecord } from "./db";
import {
    RelayConnection,
    parseEndpoint,
    type RelayEndpoint,
} from "./transport";
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

export interface Toast {
    id: string;
    text: string;
    kind: "info" | "success" | "error";
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

export function createTolooClient() {
    const [initialized, setInitialized] = createSignal(false);
    const [identity, setIdentity] = createSignal<Identity | null>(null);
    const [relays, setRelays] = createSignal<RelayRecord[]>([]);
    const [rooms, setRooms] = createSignal<RoomRecord[]>([]);
    const [activePub, setActivePub] = createSignal<string | null>(null);
    const [messages, setMessages] = createSignal<Message[]>([]);
    const [members, setMembers] = createSignal<string[]>([]);
    const [banned, setBanned] = createSignal<string[]>([]);
    const [syncState, setSyncState] = createSignal<SyncState>("idle");
    const [syncing, setSyncing] = createSignal(false);
    const [lastSync, setLastSync] = createSignal<number | null>(null);
    const [toasts, setToasts] = createSignal<Toast[]>([]);

    const activeRoom = createMemo(
        () => rooms().find((room) => room.pub === activePub()) ?? null,
    );

    const connections = new Map<string, RelayConnection>();

    function notify(
        text: string,
        kind: Toast["kind"] = "info",
        timeoutMs = 4200,
    ): void {
        const id = randomId();
        setToasts((current) => [...current, { id, text, kind }]);
        window.setTimeout(() => {
            setToasts((current) => current.filter((toast) => toast.id !== id));
        }, timeoutMs);
    }

    function dismissToast(id: string): void {
        setToasts((current) => current.filter((toast) => toast.id !== id));
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

    async function load(): Promise<void> {
        const [savedIdentity, savedRelays, savedRooms] = await Promise.all([
            db.getIdentity(),
            db.listRelays(),
            db.listRooms(),
        ]);
        setIdentity(savedIdentity ?? null);
        setRelays(savedRelays);
        setRooms(savedRooms);
        if (savedIdentity && savedRooms.length > 0) {
            setActivePub(savedRooms[0].pub);
            await refreshRoom(savedRooms[0].pub);
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

        const best = new Map<string, EnvelopeRecord>();
        for (const record of records) {
            if (record.type !== "room.message") continue;
            if (!record.body) continue;
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
        setMessages(list);

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
            notify("Message saved locally. Add a relay to send it.", "info");
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

    async function removeRoom(roomPub: string): Promise<void> {
        await db.deleteRoom(roomPub);
        setRooms((current) => {
            const remaining = current.filter((room) => room.pub !== roomPub);
            if (activePub() === roomPub) {
                const next = remaining[0]?.pub ?? null;
                setActivePub(next);
                if (next) void refreshRoom(next);
                else {
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

    async function reset(): Promise<void> {
        for (const id of connections.keys()) closeConnection(id);
        await db.clearLocalData();
        setIdentity(null);
        setRooms([]);
        setRelays([]);
        setActivePub(null);
        setMessages([]);
        setMembers([]);
        setSyncState("idle");
        setLastSync(null);
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
        activePub,
        activeRoom,
        messages,
        members,
        banned,
        syncState,
        syncing,
        lastSync,
        toasts,
        load,
        createIdentity,
        restoreIdentity,
        addRelay,
        removeRelay,
        createRoom,
        joinRoom,
        selectRoom,
        syncRoom,
        sendMessage,
        removeRoom,
        exportIdentity,
        reset,
        notify,
        dismissToast,
        startAutoSync,
    };
}

export type TolooClient = ReturnType<typeof createTolooClient>;
