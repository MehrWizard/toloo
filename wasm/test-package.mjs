// Smoke test for the generated browser package (run after build.sh).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import init, {
    PairKeyCache,
    RoomRules,
    buildMembership,
    canonicalize,
    coreVersion,
    decodeDiscovery,
    encodeUri,
    envelopeInfo,
    generateIdentity,
    generateRoomIdentity,
    importIdentity,
    makeNodeEnvelope,
    makeRoomCreate,
    makeRoomJoin,
    makeRoomMessage,
    privateDecrypt,
    privateEncrypt,
    requestEventsSync,
    sha256Hex,
    validateEvent,
    verifyChain,
    wrapCommit,
} from "./pkg/toloo_wasm.js";

const wasm = await readFile(new URL("./pkg/toloo_wasm_bg.wasm", import.meta.url));
await init({ module_or_path: wasm });

assert.equal(coreVersion(), "0.2");

const alice = JSON.parse(generateIdentity());
const bob = JSON.parse(generateIdentity());
assert.deepEqual(JSON.parse(importIdentity(alice.sigSeed, alice.encPriv)), alice);

const envelope = makeNodeEnvelope(
    "node.meta",
    JSON.stringify({ name: "smoke" }),
    alice.sigPub,
    alice.sigSeed,
);
assert.deepEqual(JSON.parse(verifyChain(envelope)), { ok: true });
const info = JSON.parse(envelopeInfo(envelope));
assert.equal(info.type, "node.meta");
assert.equal(info.depth, 1);
assert.equal(info.signer, alice.sigPub);

const cipher = privateEncrypt(
    JSON.stringify({ body: "hello" }),
    alice.encPriv,
    alice.encPub,
    bob.encPub,
);
const plaintext = JSON.parse(privateDecrypt(cipher, bob.encPriv, bob.encPub));
assert.equal(plaintext.body, "hello");

const aliceCache = new PairKeyCache(8);
const bobCache = new PairKeyCache(8);
const cachedCipher = aliceCache.encrypt(
    JSON.stringify({ body: "cached" }),
    alice.encPriv,
    alice.encPub,
    bob.encPub,
);
const cachedPlaintext = JSON.parse(
    bobCache.decrypt(cachedCipher, bob.encPriv, bob.encPub),
);
assert.equal(cachedPlaintext.body, "cached");

assert.equal(canonicalize('{ "b": 2, "a": 1 }'), '{"a":1,"b":2}');

const aliceJson = JSON.stringify(alice);
const room = JSON.parse(generateRoomIdentity());
const createdPair = JSON.parse(
    makeRoomCreate(aliceJson, JSON.stringify(room), "smoke room", null),
);
const committedJson = JSON.stringify(createdPair.committed);
assert.deepEqual(JSON.parse(verifyChain(committedJson)), { ok: true });
assert.deepEqual(JSON.parse(validateEvent(committedJson)), { ok: true });

const message = makeRoomMessage(aliceJson, room.sigPub, 1, "hello", null);
assert.deepEqual(JSON.parse(verifyChain(message)), { ok: true });
const messageInfo = JSON.parse(envelopeInfo(message));
assert.equal(messageInfo.type, "room.message");
assert.equal(messageInfo.channel, 1);

const sync = requestEventsSync(aliceJson, JSON.stringify([room.sigPub]), 0, 10);
assert.equal(JSON.parse(envelopeInfo(sync)).type, "events.sync");

const uri = encodeUri(JSON.stringify([JSON.parse(message)]));
assert.ok(uri.startsWith("toloo://"));
assert.equal(JSON.parse(decodeDiscovery(uri)).length, 1);

assert.equal(
    sha256Hex(new Uint8Array()),
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
);

const join = makeRoomJoin(aliceJson, room.sigPub, null);
const committedJoin = wrapCommit(join, JSON.stringify(room), Date.now());
const membership = JSON.parse(
    buildMembership(room.sigPub, JSON.stringify([JSON.parse(committedJoin)])),
);
assert.deepEqual(membership.members, [alice.sigPub]);
assert.deepEqual(membership.banned, []);

const rules = new RoomRules(
    JSON.stringify([
        { t: "join", allow: "*" },
        { t: "post", allow: "members" },
    ]),
);
assert.equal(rules.canJoin(alice.sigPub, "[]"), true);
assert.equal(rules.canPost(alice.sigPub, "[]"), false);
assert.equal(
    rules.canPost(alice.sigPub, JSON.stringify([alice.sigPub])),
    true,
);

console.log("wasm package smoke test ok");
