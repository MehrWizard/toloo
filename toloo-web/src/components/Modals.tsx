import {
    For,
    Show,
    createSignal,
    type JSX,
} from "solid-js";
import type { RoomRecord } from "../lib/db";
import type { TolooClient } from "../lib/client";
import { defaultEndpointScheme } from "../lib/transport";
import { decryptIdentity, encryptIdentity } from "../lib/identity";
import { copyText, downloadText, shortKey } from "../lib/ui";
import * as protocol from "../lib/wasm";
import { SETTINGS_SECTIONS } from "../generated/design";
import { CopyIcon, TrashIcon } from "./Icons";
import { Avatar } from "./Avatar";
import { Modal } from "./Modal";

export function SettingsModal(props: {
    client: TolooClient;
    onClose: () => void;
}): JSX.Element {
    const [relayUrl, setRelayUrl] = createSignal("");
    const [error, setError] = createSignal("");
    const [busy, setBusy] = createSignal(false);
    const [rtl, setRtl] = createSignal(
        localStorage.getItem("toloo-rtl") === "1",
    );
    const [notificationsEnabled, setNotificationsEnabled] = createSignal(
        localStorage.getItem("toloo-notifications") === "1" ||
            (typeof Notification !== "undefined" &&
                Notification.permission === "granted" &&
                localStorage.getItem("toloo-notifications") !== "0"),
    );
    const [passphrase, setPassphrase] = createSignal("");
    const [sigSeed, setSigSeed] = createSignal("");
    const [encPriv, setEncPriv] = createSignal("");
    const [identityJson, setIdentityJson] = createSignal("");
    const [encryptedBlob, setEncryptedBlob] = createSignal("");

    function sectionTitle(id: string): string {
        return (
            SETTINGS_SECTIONS.find((section) => section.id === id)?.title ?? id
        );
    }

    async function run(action: () => Promise<void>): Promise<void> {
        setError("");
        setBusy(true);
        try {
            await action();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    async function downloadEncryptedIdentity(): Promise<void> {
        await run(async () => {
            const blob = await encryptIdentity(
                props.client.exportIdentity(),
                passphrase(),
            );
            downloadText(blob, "toloo-identity.toloo-key", "text/plain");
            props.client.notify("Encrypted identity downloaded.", "success");
        });
    }

    async function unlockEncryptedIdentity(): Promise<void> {
        await run(async () => {
            const identityJson = await decryptIdentity(
                encryptedBlob(),
                passphrase(),
            );
            await props.client.restoreIdentityJson(identityJson);
            props.client.notify("Encrypted identity unlocked.", "success");
        });
    }

    async function toggleNotifications(): Promise<void> {
        if (notificationsEnabled()) {
            setNotificationsEnabled(false);
            localStorage.setItem("toloo-notifications", "0");
            return;
        }
        if (typeof Notification === "undefined") {
            props.client.notify("Notifications are unavailable here.", "error");
            return;
        }
        const permission = await Notification.requestPermission();
        const enabled = permission === "granted";
        setNotificationsEnabled(enabled);
        localStorage.setItem("toloo-notifications", enabled ? "1" : "0");
        if (!enabled) {
            props.client.notify("Notification permission was not granted.", "error");
        }
    }

    function toggleRtl(): void {
        const next = !rtl();
        setRtl(next);
        localStorage.setItem("toloo-rtl", next ? "1" : "0");
        document.documentElement.dir = next ? "rtl" : "ltr";
    }

    const placeholder = () =>
        `${defaultEndpointScheme()}://relay.example/toloo`;

    async function addRelay(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        await run(async () => {
            await props.client.addRelay(relayUrl());
            setRelayUrl("");
        });
    }


    async function removeIdentity(): Promise<void> {
        if (!window.confirm("Remove the identity from this device?")) return;
        await props.client.removeIdentity();
        props.onClose();
    }

    const identity = () => props.client.identity();

    return (
        <Modal
            title="Identity & Settings"
            onClose={props.onClose}
            wide
        >
            <div class="settings-grid">
                <section class="settings-section">
                    <h3>{sectionTitle("identity")}</h3>
                    <div class="identity-summary">
                        <Avatar
                            publicKey={identity()?.sigPub ?? ""}
                            size={44}
                            label="This device"
                        />
                        <div class="identity-summary-copy">
                            <span class="identity-summary-title">
                                {identity() ? "Identity loaded" : "No identity"}
                            </span>
                            <span class="identity-summary-key">
                                {identity()
                                    ? shortKey(identity()!.sigPub, 8)
                                    : "No identity"}
                            </span>
                        </div>
                    </div>
                    <Show
                        when={identity()}
                        fallback={
                            <>
                                <input
                                    type="password"
                                    autocomplete="off"
                                    placeholder="Signing seed"
                                    value={sigSeed()}
                                    onInput={(event) =>
                                        setSigSeed(event.currentTarget.value)
                                    }
                                />
                                <input
                                    type="password"
                                    autocomplete="off"
                                    placeholder="Encryption private key"
                                    value={encPriv()}
                                    onInput={(event) =>
                                        setEncPriv(event.currentTarget.value)
                                    }
                                />
                                <div class="button-row">
                                    <button
                                        type="button"
                                        class="primary-button"
                                        disabled={busy()}
                                        onClick={() =>
                                            void run(async () => {
                                                await props.client.createIdentity();
                                            })
                                        }
                                    >
                                        Create identity
                                    </button>
                                    <button
                                        type="button"
                                        class="ghost-button"
                                        disabled={
                                            busy() ||
                                            !sigSeed() ||
                                            !encPriv()
                                        }
                                        onClick={() =>
                                            void run(async () => {
                                                await props.client.restoreIdentity(
                                                    sigSeed(),
                                                    encPriv(),
                                                );
                                            })
                                        }
                                    >
                                        Restore
                                    </button>
                                </div>
                                <input
                                    type="text"
                                    spellcheck={false}
                                    placeholder="Paste identity JSON to import"
                                    value={identityJson()}
                                    onInput={(event) =>
                                        setIdentityJson(
                                            event.currentTarget.value,
                                        )
                                    }
                                />
                                <button
                                    type="button"
                                    class="ghost-button wide"
                                    disabled={busy() || !identityJson()}
                                    onClick={() =>
                                        void run(async () => {
                                            await props.client.restoreIdentityJson(
                                                identityJson(),
                                            );
                                        })
                                    }
                                >
                                    Import identity JSON
                                </button>
                            </>
                        }
                    >
                        <label class="identity-key-block">
                            <span>SIGNING KEY</span>
                            <code>{identity()?.sigPub}</code>
                        </label>
                        <label class="identity-key-block">
                            <span>ENCRYPTION KEY</span>
                            <code>{identity()?.encPub}</code>
                        </label>
                    </Show>
                </section>

                <section class="settings-section">
                    <h3>{sectionTitle("encryptedIdentity")}</h3>
                    <p class="help">
                        Wrap both private keys with AES-256-GCM using a
                        passphrase-derived key. The .toloo-key file is safe to
                        store separately; the passphrase is not recoverable.
                    </p>
                    <input
                        type="password"
                        autocomplete="new-password"
                        placeholder="Passphrase"
                        value={passphrase()}
                        onInput={(event) =>
                            setPassphrase(event.currentTarget.value)
                        }
                    />
                    <div class="button-row">
                        <button
                            type="button"
                            class="ghost-button"
                            disabled={busy() || !identity()}
                            onClick={() =>
                                downloadText(
                                    props.client.exportIdentity(),
                                    "toloo-identity.json",
                                )
                            }
                        >
                            Export JSON
                        </button>
                        <button
                            type="button"
                            class="primary-button"
                            disabled={busy() || !identity() || !passphrase()}
                            onClick={() => void downloadEncryptedIdentity()}
                        >
                            Encrypted export
                        </button>
                    </div>
                    <input
                        type="text"
                        spellcheck={false}
                        placeholder="Encrypted identity blob"
                        value={encryptedBlob()}
                        onInput={(event) =>
                            setEncryptedBlob(event.currentTarget.value)
                        }
                    />
                    <button
                        type="button"
                        class="ghost-button wide"
                        disabled={busy() || !encryptedBlob() || !passphrase()}
                        onClick={() => void unlockEncryptedIdentity()}
                    >
                        Unlock encrypted identity
                    </button>
                </section>

                <section class="settings-section">
                    <h3>{sectionTitle("relays")}</h3>
                    <p class="relay-network">
                        {props.client.relays().length === 0
                            ? "No relays"
                            : props.client.relays().length === 1
                              ? "1 relay"
                              : `${props.client.relays().length} relays`}
                    </p>
                    <Show when={props.client.relays().length > 0}>
                        <ul class="relay-list">
                            <For each={props.client.relays()}>
                                {(relay) => (
                                    <li>
                                        <span class="proto-tag">{relay.proto}</span>
                                        <code title={relay.url}>{relay.url}</code>
                                        <button
                                            type="button"
                                            class="icon-button subtle"
                                            aria-label={`Remove ${relay.url}`}
                                            onClick={() => {
                                                void props.client.removeRelay(relay.id);
                                            }}
                                        >
                                            <TrashIcon size={16} />
                                        </button>
                                    </li>
                                )}
                            </For>
                        </ul>
                    </Show>
                    <form class="inline-form" onSubmit={(event) => void addRelay(event)}>
                        <input
                            type="text"
                            spellcheck={false}
                            placeholder={placeholder()}
                            value={relayUrl()}
                            onInput={(event) => setRelayUrl(event.currentTarget.value)}
                        />
                        <button
                            class="ghost-button"
                            type="submit"
                            disabled={busy() || !relayUrl().trim()}
                        >
                            Add
                        </button>
                    </form>
                    <Show when={error()}>
                        <p class="form-error" role="alert">
                            {error()}
                        </p>
                    </Show>
                </section>

                <section class="settings-section">
                    <h3>{sectionTitle("host")}</h3>
                    <p class="help">
                        Relay hosting is available in the desktop and Android
                        apps. Browsers cannot open local relay sockets.
                    </p>
                </section>

                <section class="settings-section">
                    <h3>{sectionTitle("notifications")}</h3>
                    <button
                        type="button"
                        class="ghost-button wide"
                        onClick={() => void toggleNotifications()}
                    >
                        {notificationsEnabled()
                            ? "Notifications: on"
                            : "Notifications: off"}
                    </button>
                </section>

                <section class="settings-section">
                    <h3>{sectionTitle("layout")}</h3>
                    <button
                        type="button"
                        class="ghost-button"
                        onClick={toggleRtl}
                    >
                        {rtl()
                            ? "Right-to-left: on"
                            : "Right-to-left: off"}
                    </button>
                </section>

                <section class="settings-section">
                    <h3>{sectionTitle("blocked")}</h3>
                    <Show
                        when={props.client.blocked().length > 0}
                        fallback={<p class="empty-note">No blocked nodes.</p>}
                    >
                        <ul class="relay-list">
                            <For each={props.client.blocked()}>
                                {(node) => (
                                    <li>
                                        <code title={node}>{shortKey(node, 10)}</code>
                                        <button
                                            type="button"
                                            class="ghost-button compact"
                                            onClick={() => {
                                                void props.client.unblockAuthor(node);
                                            }}
                                        >
                                            Unblock
                                        </button>
                                    </li>
                                )}
                            </For>
                        </ul>
                    </Show>
                </section>

                <section class="settings-section danger-zone">
                    <h3>{sectionTitle("device")}</h3>
                    <p class="help">
                        {identity()
                            ? "This identity is stored on this device."
                            : "No identity is stored on this device."}
                    </p>
                    <Show when={identity()}>
                        <button
                            type="button"
                            class="danger-button wide"
                            onClick={() => void removeIdentity()}
                        >
                            Remove identity from this device
                        </button>
                    </Show>
                </section>
            </div>
        </Modal>
    );
}

export function AddRoomModal(props: {
    client: TolooClient;
    onClose: () => void;
    onCreated: (room: RoomRecord) => void;
}): JSX.Element {
    const [name, setName] = createSignal("");
    const [joinInput, setJoinInput] = createSignal("");
    const [error, setError] = createSignal("");
    const [busy, setBusy] = createSignal(false);

    async function create(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        setError("");
        setBusy(true);
        try {
            const room = await props.client.createRoom(name());
            props.onCreated(room);
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    async function join(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        setError("");
        setBusy(true);
        try {
            await props.client.joinRoom(joinInput());
            props.onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <Modal title="New Room" onClose={props.onClose}>
            <div class="field-label">CREATE</div>
            <form class="stack" onSubmit={(event) => void create(event)}>
                <input
                    type="text"
                    placeholder="Room name (optional)"
                    value={name()}
                    onInput={(event) => setName(event.currentTarget.value)}
                />
                <button
                    class="primary-button wide"
                    type="submit"
                    disabled={busy()}
                >
                    Create Room
                </button>
            </form>

            <div class="divider" />
            <div class="field-label">JOIN</div>
            <form class="stack" onSubmit={(event) => void join(event)}>
                <input
                    type="text"
                    spellcheck={false}
                    placeholder="toloo:// invite or room key"
                    value={joinInput()}
                    onInput={(event) => setJoinInput(event.currentTarget.value)}
                />
                <button
                    class="ghost-button wide"
                    type="submit"
                    disabled={busy() || !joinInput().trim()}
                >
                    Join Room
                </button>
            </form>

            <Show when={error()}>
                <p class="form-error" role="alert">
                    {error()}
                </p>
            </Show>
        </Modal>
    );
}

export function ImportModal(props: {
    client: TolooClient;
    onClose: () => void;
}): JSX.Element {
    const [input, setInput] = createSignal("");
    const [error, setError] = createSignal("");
    const [busy, setBusy] = createSignal(false);

    async function decode(): Promise<void> {
        setError("");
        setBusy(true);
        try {
            await props.client.importDiscovery(input());
            props.onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <Modal title="Import" onClose={props.onClose}>
            <p class="help">
                Paste a toloo:// link or open a .toloo file. Rooms, messages,
                and relay endpoints are detected automatically.
            </p>
            <input
                type="text"
                spellcheck={false}
                placeholder="toloo://BASE64URL…"
                value={input()}
                onInput={(event) => setInput(event.currentTarget.value)}
            />
            <div class="button-row">
                <button
                    class="primary-button"
                    type="button"
                    disabled={busy() || !input().trim()}
                    onClick={() => void decode()}
                >
                    Decode
                </button>
                <button
                    class="ghost-button"
                    type="button"
                    onClick={props.onClose}
                >
                    Close
                </button>
            </div>
            <Show when={error()}>
                <p class="form-error" role="alert">
                    {error()}
                </p>
            </Show>
        </Modal>
    );
}

export function RenameModal(props: {
    client: TolooClient;
    room: RoomRecord;
    onClose: () => void;
}): JSX.Element {
    const [name, setName] = createSignal("");
    const [error, setError] = createSignal("");
    const [busy, setBusy] = createSignal(false);

    async function rename(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        setError("");
        setBusy(true);
        try {
            await props.client.renameRoom(name(), props.room.pub);
            props.onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <Modal title="Rename room" onClose={props.onClose}>
            <p class="help">{props.room.name}</p>
            <form class="stack" onSubmit={(event) => void rename(event)}>
                <input
                    type="text"
                    placeholder="New room name"
                    value={name()}
                    onInput={(event) => setName(event.currentTarget.value)}
                />
                <div class="button-row">
                    <button
                        class="primary-button"
                        type="submit"
                        disabled={busy() || !name().trim()}
                    >
                        Rename
                    </button>
                    <button
                        class="ghost-button"
                        type="button"
                        onClick={props.onClose}
                    >
                        Cancel
                    </button>
                </div>
            </form>
            <Show when={error()}>
                <p class="form-error" role="alert">
                    {error()}
                </p>
            </Show>
        </Modal>
    );
}

export function InviteModal(props: {
    client: TolooClient;
    room: RoomRecord;
    onClose: () => void;
}): JSX.Element {
    const invite = () => {
        if (props.room.inviteUri) return props.room.inviteUri;
        if (!props.room.roomCreate) return "";
        try {
            return protocol.encodeInvite([
                JSON.parse(props.room.roomCreate) as protocol.Envelope,
            ]);
        } catch {
            return "";
        }
    };

    async function copyInvite(): Promise<void> {
        const uri = invite();
        if (!uri) return;
        try {
            await copyText(uri);
            props.client.notify("Invite copied.", "success");
        } catch {
            props.client.notify("Could not copy the invite.", "error");
        }
    }

    async function shareInvite(): Promise<void> {
        const text = invite();
        if (!text) return;
        if (navigator.share) {
            try {
                await navigator.share({ text });
                return;
            } catch {
                // Fall back to the clipboard when the share sheet is closed.
            }
        }
        await copyInvite();
    }

    return (
        <Modal title="Share invite" onClose={props.onClose}>
            <p class="help">
                Keep this text secret. Select it below to copy.
            </p>
            <Show
                when={invite()}
                fallback={
                    <p class="help">
                        No signed room.create is stored for this room. Share the
                        room public key instead:
                    </p>
                }
            >
                <textarea
                    class="invite-box"
                    rows={5}
                    readonly
                    value={invite()}
                    onFocus={(event) => event.currentTarget.select()}
                />
            </Show>
            <p class="room-key-line">{props.room.pub}</p>
            <div class="button-row">
                <button
                    type="button"
                    class="ghost-button"
                    disabled={!invite()}
                    onClick={() => void copyInvite()}
                >
                    <CopyIcon size={15} />
                    Copy
                </button>
                <button
                    type="button"
                    class="primary-button"
                    disabled={!invite()}
                    onClick={() => void shareInvite()}
                >
                    Share
                </button>
            </div>
            <button type="button" class="ghost-button wide" onClick={props.onClose}>
                Done
            </button>
        </Modal>
    );
}
