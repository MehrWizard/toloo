import type { Identity, RoomIdentity } from "./wasm";
import type { TransportProto } from "./transport";

const DB_NAME = "toloo-web";
const DB_VERSION = 1;
const KV = "kv";
const ROOMS = "rooms";
const RELAYS = "relays";
const ENVELOPES = "envelopes";

export interface RoomRecord {
    pub: string;
    name: string;
    createdAt: number;
    inviteUri?: string;
    roomIdentity?: RoomIdentity;
    roomCreate?: string;
    lastReadAt?: number;
}

export interface RelayRecord {
    id: string;
    url: string;
    proto: TransportProto;
    createdAt: number;
}

export interface EnvelopeRecord {
    datumId: string;
    eid: string;
    envelope: string;
    room: string | null;
    to: string | null;
    type: string;
    channel: number | null;
    ts: number;
    tc: number | null;
    depth: number;
    signer: string;
    body: string | null;
}

let dbPromise: Promise<IDBDatabase> | null = null;

function openDb(): Promise<IDBDatabase> {
    if (dbPromise) return dbPromise;
    dbPromise = new Promise((resolve, reject) => {
        const request = indexedDB.open(DB_NAME, DB_VERSION);
        request.onupgradeneeded = () => {
            const db = request.result;
            if (!db.objectStoreNames.contains(KV)) {
                db.createObjectStore(KV, { keyPath: "key" });
            }
            if (!db.objectStoreNames.contains(ROOMS)) {
                db.createObjectStore(ROOMS, { keyPath: "pub" });
            }
            if (!db.objectStoreNames.contains(RELAYS)) {
                db.createObjectStore(RELAYS, { keyPath: "id" });
            }
            if (!db.objectStoreNames.contains(ENVELOPES)) {
                const store = db.createObjectStore(ENVELOPES, {
                    keyPath: "datumId",
                });
                store.createIndex("room", "room");
                store.createIndex("eid", "eid");
                store.createIndex("type", "type");
            }
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () =>
            reject(request.error ?? new Error("Could not open IndexedDB."));
    });
    return dbPromise;
}

function requestResult<T>(request: IDBRequest<T>): Promise<T> {
    return new Promise((resolve, reject) => {
        request.onsuccess = () => resolve(request.result);
        request.onerror = () =>
            reject(request.error ?? new Error("IndexedDB request failed."));
    });
}

function transactionDone(tx: IDBTransaction): Promise<void> {
    return new Promise((resolve, reject) => {
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error ?? new Error("IndexedDB write failed."));
        tx.onabort = () => reject(tx.error ?? new Error("IndexedDB write aborted."));
    });
}

export async function getIdentity(): Promise<Identity | undefined> {
    const db = await openDb();
    const tx = db.transaction(KV, "readonly");
    const result = await requestResult<{ key: string; value: Identity } | undefined>(
        tx.objectStore(KV).get("identity"),
    );
    return result?.value;
}

export async function getVault(): Promise<string | undefined> {
    const db = await openDb();
    const tx = db.transaction(KV, "readonly");
    const result = await requestResult<{ key: string; value: string } | undefined>(
        tx.objectStore(KV).get("vault"),
    );
    return result?.value;
}

export async function saveVault(blob: string): Promise<void> {
    const db = await openDb();
    const tx = db.transaction(KV, "readwrite");
    tx.objectStore(KV).put({ key: "vault", value: blob });
    await transactionDone(tx);
}

export async function deleteVault(): Promise<void> {
    const db = await openDb();
    const tx = db.transaction(KV, "readwrite");
    tx.objectStore(KV).delete("vault");
    await transactionDone(tx);
}

export async function deleteIdentity(): Promise<void> {
    const db = await openDb();
    const tx = db.transaction(KV, "readwrite");
    tx.objectStore(KV).delete("identity");
    await transactionDone(tx);
}

export async function getSetting<T>(key: string): Promise<T | undefined> {
    const db = await openDb();
    const tx = db.transaction(KV, "readonly");
    const result = await requestResult<{ key: string; value: T } | undefined>(
        tx.objectStore(KV).get(key),
    );
    return result?.value;
}

export async function saveSetting<T>(key: string, value: T): Promise<void> {
    const db = await openDb();
    const tx = db.transaction(KV, "readwrite");
    tx.objectStore(KV).put({ key, value });
    await transactionDone(tx);
}

export async function listRooms(): Promise<RoomRecord[]> {
    const db = await openDb();
    const tx = db.transaction(ROOMS, "readonly");
    const rooms = await requestResult<RoomRecord[]>(tx.objectStore(ROOMS).getAll());
    return rooms.sort((a, b) => a.createdAt - b.createdAt);
}

export async function saveRoom(room: RoomRecord): Promise<void> {
    const db = await openDb();
    const tx = db.transaction(ROOMS, "readwrite");
    const metadata = { ...room };
    delete metadata.roomIdentity;
    tx.objectStore(ROOMS).put(metadata);
    await transactionDone(tx);
}

export async function deleteRoom(pub: string): Promise<void> {
    const db = await openDb();
    const tx = db.transaction([ROOMS, ENVELOPES], "readwrite");
    tx.objectStore(ROOMS).delete(pub);
    const index = tx.objectStore(ENVELOPES).index("room");
    const cursor = index.openCursor(IDBKeyRange.only(pub));
    cursor.onsuccess = () => {
        const value = cursor.result;
        if (!value) return;
        value.delete();
        value.continue();
    };
    await transactionDone(tx);
}

export async function listRelays(): Promise<RelayRecord[]> {
    const db = await openDb();
    const tx = db.transaction(RELAYS, "readonly");
    const relays = await requestResult<RelayRecord[]>(
        tx.objectStore(RELAYS).getAll(),
    );
    return relays.sort((a, b) => a.createdAt - b.createdAt);
}

export async function saveRelay(relay: RelayRecord): Promise<void> {
    const db = await openDb();
    const tx = db.transaction(RELAYS, "readwrite");
    tx.objectStore(RELAYS).put(relay);
    await transactionDone(tx);
}

export async function deleteRelay(id: string): Promise<void> {
    const db = await openDb();
    const tx = db.transaction(RELAYS, "readwrite");
    tx.objectStore(RELAYS).delete(id);
    await transactionDone(tx);
}

export async function putEnvelopes(records: EnvelopeRecord[]): Promise<void> {
    if (records.length === 0) return;
    const db = await openDb();
    const tx = db.transaction(ENVELOPES, "readwrite");
    const store = tx.objectStore(ENVELOPES);
    for (const record of records) {
        store.put(record);
    }
    await transactionDone(tx);
}

export async function roomEnvelopes(room: string): Promise<EnvelopeRecord[]> {
    const db = await openDb();
    const tx = db.transaction(ENVELOPES, "readonly");
    const index = tx.objectStore(ENVELOPES).index("room");
    return requestResult<EnvelopeRecord[]>(index.getAll(IDBKeyRange.only(room)));
}

export async function allEnvelopes(): Promise<EnvelopeRecord[]> {
    const db = await openDb();
    const tx = db.transaction(ENVELOPES, "readonly");
    return requestResult<EnvelopeRecord[]>(tx.objectStore(ENVELOPES).getAll());
}

export async function clearLocalData(): Promise<void> {
    const db = await openDb();
    const tx = db.transaction([KV, ROOMS, RELAYS, ENVELOPES], "readwrite");
    tx.objectStore(KV).clear();
    tx.objectStore(ROOMS).clear();
    tx.objectStore(RELAYS).clear();
    tx.objectStore(ENVELOPES).clear();
    await transactionDone(tx);
}
