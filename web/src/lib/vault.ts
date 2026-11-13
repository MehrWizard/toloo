import { decryptIdentity, encryptIdentity } from "./identity";
import type { Identity, RoomIdentity } from "./wasm";

export interface VaultPayload {
    version: 1;
    identity: Identity;
    roomKeys: Record<string, RoomIdentity>;
}

function parseVault(raw: string): VaultPayload {
    let value: unknown;
    try {
        value = JSON.parse(raw);
    } catch {
        throw new Error("The local vault is corrupted.");
    }
    if (!value || typeof value !== "object") {
        throw new Error("The local vault is corrupted.");
    }
    const vault = value as Partial<VaultPayload>;
    if (
        vault.version !== 1 ||
        !vault.identity ||
        typeof vault.identity.sigSeed !== "string" ||
        typeof vault.identity.encPriv !== "string" ||
        !vault.roomKeys ||
        typeof vault.roomKeys !== "object"
    ) {
        throw new Error("The local vault is corrupted.");
    }
    return {
        version: 1,
        identity: vault.identity,
        roomKeys: vault.roomKeys,
    };
}

export async function encryptVault(
    identity: Identity,
    roomKeys: Record<string, RoomIdentity>,
    passphrase: string,
): Promise<string> {
    const payload: VaultPayload = { version: 1, identity, roomKeys };
    return encryptIdentity(JSON.stringify(payload), passphrase);
}

export async function decryptVault(
    blob: string,
    passphrase: string,
): Promise<VaultPayload> {
    return parseVault(await decryptIdentity(blob, passphrase));
}
