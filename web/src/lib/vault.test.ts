import { describe, expect, it } from "vitest";
import type { Identity, RoomIdentity } from "./wasm";
import { decryptVault, encryptVault } from "./vault";

const identity: Identity = {
    sigPub: "sig-public",
    sigSeed: "sig-private-seed",
    encPub: "enc-public",
    encPriv: "enc-private-key",
};

const roomKeys: Record<string, RoomIdentity> = {
    "room-public": {
        sigPub: "room-public",
        sigSeed: "room-private-seed",
    },
};

describe("local vault", () => {
    it("round-trips identity and room keys", async () => {
        const blob = await encryptVault(identity, roomKeys, "correct horse");
        const payload = await decryptVault(blob, "correct horse");
        expect(payload.identity).toEqual(identity);
        expect(payload.roomKeys).toEqual(roomKeys);
    });

    it("rejects the wrong passphrase", async () => {
        const blob = await encryptVault(identity, roomKeys, "correct horse");
        await expect(decryptVault(blob, "wrong")).rejects.toThrow(
            /Wrong passphrase/,
        );
    });
});
