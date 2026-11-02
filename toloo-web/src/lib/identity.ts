const PREFIX = "toloo-key:v1:";
const ITERATIONS = 210_000;

type Bytes = Uint8Array<ArrayBuffer>;

function toBase64(bytes: Uint8Array): string {
    let binary = "";
    for (const byte of bytes) binary += String.fromCharCode(byte);
    return btoa(binary);
}

function fromBase64(value: string): Bytes {
    const binary = atob(value);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
    }
    return bytes;
}

async function deriveKey(
    passphrase: string,
    salt: Bytes,
): Promise<CryptoKey> {
    const material = await crypto.subtle.importKey(
        "raw",
        new TextEncoder().encode(passphrase),
        "PBKDF2",
        false,
        ["deriveKey"],
    );
    return crypto.subtle.deriveKey(
        { name: "PBKDF2", salt, iterations: ITERATIONS, hash: "SHA-256" },
        material,
        { name: "AES-GCM", length: 256 },
        false,
        ["encrypt", "decrypt"],
    );
}

export async function encryptIdentity(
    identityJson: string,
    passphrase: string,
): Promise<string> {
    if (!passphrase) throw new Error("Enter a passphrase.");
    const salt = crypto.getRandomValues(new Uint8Array(16));
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const key = await deriveKey(passphrase, salt);
    const ciphertext = new Uint8Array(
        await crypto.subtle.encrypt(
            { name: "AES-GCM", iv },
            key,
            new TextEncoder().encode(identityJson),
        ),
    );
    const blob = new Uint8Array(salt.length + iv.length + ciphertext.length);
    blob.set(salt, 0);
    blob.set(iv, salt.length);
    blob.set(ciphertext, salt.length + iv.length);
    return `${PREFIX}${toBase64(blob)}`;
}

export async function decryptIdentity(
    blob: string,
    passphrase: string,
): Promise<string> {
    const trimmed = blob.trim();
    if (!trimmed.startsWith(PREFIX)) {
        throw new Error("Not a Toloo encrypted identity file.");
    }
    const raw = fromBase64(trimmed.slice(PREFIX.length));
    if (raw.length < 16 + 12 + 16) {
        throw new Error("Encrypted identity is corrupted.");
    }
    const salt = raw.slice(0, 16);
    const iv = raw.slice(16, 28);
    const ciphertext = raw.slice(28);
    const key = await deriveKey(passphrase, salt);
    try {
        const plaintext = await crypto.subtle.decrypt(
            { name: "AES-GCM", iv },
            key,
            ciphertext,
        );
        return new TextDecoder().decode(plaintext);
    } catch {
        throw new Error("Wrong passphrase or corrupted identity file.");
    }
}
