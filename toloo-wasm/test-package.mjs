// Smoke test for the generated browser package (run after build.sh).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import init, {
    PairKeyCache,
    canonicalize,
    coreVersion,
    envelopeInfo,
    generateIdentity,
    importIdentity,
    makeNodeEnvelope,
    privateDecrypt,
    privateEncrypt,
    verifyChain,
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

console.log("wasm package smoke test ok");
