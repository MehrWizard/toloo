import { describe, expect, it } from "vitest";
import { decryptIdentity, encryptIdentity } from "./identity";

const identity = JSON.stringify({
    sigPub: "sig-public",
    sigSeed: "sig-private-seed",
    encPub: "enc-public",
    encPriv: "enc-private-key",
});

describe("encrypted identity", () => {
    it("round-trips with the right passphrase", async () => {
        const blob = await encryptIdentity(identity, "correct horse");
        expect(blob.startsWith("toloo-key:v1:")).toBe(true);
        expect(await decryptIdentity(blob, "correct horse")).toBe(identity);
    });

    it("rejects the wrong passphrase", async () => {
        const blob = await encryptIdentity(identity, "correct horse");
        await expect(decryptIdentity(blob, "wrong")).rejects.toThrow(
            /Wrong passphrase/,
        );
    });

    it("rejects malformed blobs", async () => {
        await expect(decryptIdentity("not-a-key", "x")).rejects.toThrow(
            /Not a Toloo/,
        );
    });
});
