import {
    For,
    Show,
    createSignal,
    type JSX,
} from "solid-js";
import type { RoomRecord } from "../lib/db";
import type { Message, TolooClient } from "../lib/client";
import {
    allowedProtocols,
    defaultEndpointScheme,
} from "../lib/transport";
import { decryptIdentity, encryptIdentity } from "../lib/identity";
import { copyText, downloadText, formatTime, shortKey } from "../lib/ui";
import * as protocol from "../lib/wasm";
import { CopyIcon, TrashIcon } from "./Icons";
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
    const [notifyPermission, setNotifyPermission] = createSignal(
        typeof Notification === "undefined"
            ? "denied"
            : Notification.permission,
    );

    const [passphrase, setPassphrase] = createSignal("");

    async function downloadEncryptedIdentity(): Promise<void> {
        setError("");
        setBusy(true);
        try {
            const blob = await encryptIdentity(
                props.client.exportIdentity(),
                passphrase(),
            );
            downloadText(blob, "toloo-identity.toloo-key", "text/plain");
            props.client.notify("Encrypted identity downloaded.", "success");
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    async function unlockIdentityFile(event: Event): Promise<void> {
        const input = event.currentTarget as HTMLInputElement;
        const file = input.files?.[0];
        if (!file) return;
        setError("");
        setBusy(true);
        try {
            const identityJson = await decryptIdentity(
                await file.text(),
                passphrase(),
            );
            await props.client.restoreIdentityJson(identityJson);
            props.client.notify("Encrypted identity unlocked.", "success");
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
            input.value = "";
        }
    }

    async function enableNotifications(): Promise<void> {
        if (typeof Notification === "undefined") {
            props.client.notify("Notifications are unavailable here.", "error");
            return;
        }
        const permission = await Notification.requestPermission();
        setNotifyPermission(permission);
        if (permission === "granted") {
            props.client.notify("Notifications enabled.", "success");
        } else {
            props.client.notify("Notification permission was not granted.", "error");
        }
    }

    function toggleRtl(): void {
        const next = !rtl();
        setRtl(next);
        localStorage.setItem("toloo-rtl", next ? "1" : "0");
        document.documentElement.dir = next ? "rtl" : "ltr";
    }

    const protocols = () => allowedProtocols().join(" / ");
    const placeholder = () =>
        `${defaultEndpointScheme()}://relay.example/toloo`;

    async function addRelay(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        setError("");
        setBusy(true);
        try {
            await props.client.addRelay(relayUrl());
            setRelayUrl("");
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    async function copyIdentity(): Promise<void> {
        try {
            await copyText(props.client.exportIdentity());
            props.client.notify("Recovery JSON copied.", "success");
        } catch {
            props.client.notify("Could not copy to the clipboard.", "error");
        }
    }

    async function reset(): Promise<void> {
        if (
            !window.confirm(
                "Clear the identity, rooms, relays, and cached envelopes in this browser?",
            )
        ) {
            return;
        }
        await props.client.reset();
        props.onClose();
    }

    const identity = () => props.client.identity();

    return (
        <Modal
            title="Settings"
            subtitle="Identity, relays, and this browser"
            onClose={props.onClose}
            wide
        >
            <div class="settings-grid">
                <section class="settings-section">
                    <h3>Identity</h3>
                    <div class="key-row">
                        <span>Signing</span>
                        <code>{shortKey(identity()?.sigPub ?? "", 10)}</code>
                    </div>
                    <div class="key-row">
                        <span>Encryption</span>
                        <code>{shortKey(identity()?.encPub ?? "", 10)}</code>
                    </div>
                    <p class="help">
                        The recovery JSON contains both private keys. Anyone with
                        it can impersonate this identity.
                    </p>
                    <div class="button-row">
                        <button
                            type="button"
                            class="ghost-button"
                            onClick={() => void copyIdentity()}
                        >
                            <CopyIcon size={15} />
                            Copy recovery
                        </button>
                        <button
                            type="button"
                            class="ghost-button"
                            onClick={() =>
                                downloadText(
                                    props.client.exportIdentity(),
                                    "toloo-identity.json",
                                )
                            }
                        >
                            Download
                        </button>
                    </div>
                </section>

                <section class="settings-section">
                    <h3>Encrypted identity</h3>
                    <p class="help">
                        Wrap both private keys with AES-256-GCM using a
                        passphrase-derived key. The .toloo-key file is safe to
                        store separately; the passphrase is not recoverable.
                    </p>
                    <label class="field">
                        <span>Passphrase</span>
                        <input
                            type="password"
                            autocomplete="new-password"
                            value={passphrase()}
                            onInput={(event) =>
                                setPassphrase(event.currentTarget.value)
                            }
                        />
                    </label>
                    <div class="button-row">
                        <button
                            type="button"
                            class="ghost-button"
                            disabled={busy() || !passphrase()}
                            onClick={() => void downloadEncryptedIdentity()}
                        >
                            Encrypt & download
                        </button>
                        <label class="ghost-button file-button">
                            Unlock .toloo-key
                            <input
                                type="file"
                                accept=".toloo-key,text/plain"
                                hidden
                                disabled={busy() || !passphrase()}
                                onChange={(event) => void unlockIdentityFile(event)}
                            />
                        </label>
                    </div>
                </section>

                <section class="settings-section">
                    <h3>Relays</h3>
                    <p class="help">
                        This page accepts {protocols()} endpoints.{" "}
                        {window.location.protocol === "https:"
                            ? "Self-signed relay certificates cannot be used from the browser."
                            : "HTTP pages are dev/LAN only and are not a secure context."}
                    </p>
                    <Show
                        when={props.client.relays().length > 0}
                        fallback={<p class="empty-note">No relays configured.</p>}
                    >
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
                        <button class="primary-button" type="submit" disabled={busy()}>
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
                    <h3>Blocked nodes</h3>
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

                <section class="settings-section">
                    <h3>Notifications</h3>
                    <p class="help">
                        {notifyPermission() === "granted"
                            ? "Desktop notifications are enabled for new incoming messages."
                            : "Allow desktop notifications while this tab is in the background."}
                    </p>
                    <Show when={notifyPermission() !== "granted"}>
                        <button
                            type="button"
                            class="ghost-button"
                            onClick={() => void enableNotifications()}
                        >
                            Enable notifications
                        </button>
                    </Show>
                </section>

                <section class="settings-section">
                    <h3>Layout</h3>
                    <button
                        type="button"
                        class="ghost-button"
                        onClick={toggleRtl}
                    >
                        {rtl()
                            ? "Right-to-left layout: on"
                            : "Right-to-left layout: off"}
                    </button>
                </section>

                <section class="settings-section danger-zone">
                    <h3>This browser</h3>
                    <p class="help">
                        Clear the local identity, room list, relay list, and
                        cached envelopes. Relay copies are not deleted.
                    </p>
                    <button
                        type="button"
                        class="danger-button"
                        onClick={() => void reset()}
                    >
                        Clear local data
                    </button>
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
    const [mode, setMode] = createSignal<"create" | "join" | "import">("create");
    const [name, setName] = createSignal("");
    const [invite, setInvite] = createSignal("");
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
            await props.client.joinRoom(invite());
            props.onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    async function importAll(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        setError("");
        setBusy(true);
        try {
            await props.client.importDiscovery(invite());
            props.onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <Modal
            title="Add a room"
            subtitle="Create a keypair or join with an invite"
            onClose={props.onClose}
        >
            <div class="segmented" role="tablist" aria-label="Add room">
                <button
                    type="button"
                    role="tab"
                    aria-selected={mode() === "create"}
                    classList={{ active: mode() === "create" }}
                    onClick={() => {
                        setMode("create");
                        setError("");
                    }}
                >
                    Create
                </button>
                <button
                    type="button"
                    role="tab"
                    aria-selected={mode() === "join"}
                    classList={{ active: mode() === "join" }}
                    onClick={() => {
                        setMode("join");
                        setError("");
                    }}
                >
                    Join
                </button>
                <button
                    type="button"
                    role="tab"
                    aria-selected={mode() === "import"}
                    classList={{ active: mode() === "import" }}
                    onClick={() => {
                        setMode("import");
                        setError("");
                    }}
                >
                    Import
                </button>
            </div>

            <Show when={mode() === "create"}>
                <form class="stack" onSubmit={(event) => void create(event)}>
                    <label class="field">
                        <span>Room name</span>
                        <input
                            type="text"
                            placeholder="Long table"
                            value={name()}
                            onInput={(event) => setName(event.currentTarget.value)}
                        />
                    </label>
                    <p class="help">
                        Joining and posting are open by default. The room key
                        stays in this browser, so this client can commit.
                    </p>
                    <button class="primary-button wide" type="submit" disabled={busy()}>
                        {busy() ? "Creating…" : "Create room"}
                    </button>
                </form>
            </Show>

            <Show when={mode() === "join"}>
                <form class="stack" onSubmit={(event) => void join(event)}>
                    <label class="field">
                        <span>Invite or room key</span>
                        <textarea
                            rows={4}
                            spellcheck={false}
                            placeholder="toloo://… or a 43-character room public key"
                            value={invite()}
                            onInput={(event) => setInvite(event.currentTarget.value)}
                        />
                    </label>
                    <p class="help">
                        The invite is verified locally before the room is
                        imported.
                    </p>
                    <button class="primary-button wide" type="submit" disabled={busy()}>
                        {busy() ? "Joining…" : "Join room"}
                    </button>
                </form>
            </Show>

            <Show when={mode() === "import"}>
                <form class="stack" onSubmit={(event) => void importAll(event)}>
                    <label class="field">
                        <span>toloo:// link or .toloo file contents</span>
                        <textarea
                            rows={5}
                            spellcheck={false}
                            placeholder="Paste exported rooms, messages, or node metadata"
                            value={invite()}
                            onInput={(event) => setInvite(event.currentTarget.value)}
                        />
                    </label>
                    <p class="help">
                        Envelopes are verified locally. Rooms and relay
                        endpoints are detected; existing data is kept.
                    </p>
                    <button
                        class="primary-button wide"
                        type="submit"
                        disabled={busy() || !invite().trim()}
                    >
                        {busy() ? "Importing…" : "Import everything"}
                    </button>
                </form>
            </Show>

            <Show when={error()}>
                <p class="form-error" role="alert">
                    {error()}
                </p>
            </Show>
        </Modal>
    );
}

export function RoomActionsModal(props: {
    client: TolooClient;
    room: RoomRecord;
    onClose: () => void;
}): JSX.Element {
    const [name, setName] = createSignal(props.room.name);
    const [error, setError] = createSignal("");
    const [busy, setBusy] = createSignal(false);

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

    async function leave(): Promise<void> {
        if (
            !window.confirm(
                "Leave this room? It is removed from this browser; copies on relays are kept.",
            )
        ) {
            return;
        }
        await run(async () => {
            await props.client.leaveRoom();
            props.onClose();
        });
    }

    return (
        <Modal
            title={props.room.name}
            subtitle="Room details, sharing, and moderation"
            onClose={props.onClose}
        >
            <div class="key-row room-key-row">
                <span>Room key</span>
                <code>{shortKey(props.room.pub, 12)}</code>
            </div>
            <p class="help">
                {props.room.roomIdentity
                    ? "This browser holds the room key and can commit, rename, and moderate."
                    : "You joined this room. Only the room owner can rename it."}
            </p>

            <div class="button-row">
                <Show when={invite()}>
                    <button
                        type="button"
                        class="ghost-button"
                        disabled={busy()}
                        onClick={() =>
                            void run(async () => {
                                await copyText(invite());
                                props.client.notify("Invite copied.", "success");
                            })
                        }
                    >
                        <CopyIcon size={15} />
                        Copy invite
                    </button>
                </Show>
                <button
                    type="button"
                    class="ghost-button"
                    disabled={busy()}
                    onClick={() =>
                        void run(() => props.client.exportRoomMessages(props.room.pub))
                    }
                >
                    Export messages
                </button>
                <button
                    type="button"
                    class="ghost-button"
                    disabled={busy()}
                    onClick={() => void props.client.syncRoom(props.room.pub)}
                >
                    Sync now
                </button>
            </div>

            <Show when={props.room.roomIdentity}>
                <form
                    class="inline-form"
                    onSubmit={(event) => {
                        event.preventDefault();
                        void run(async () => {
                            await props.client.renameRoom(name());
                            props.onClose();
                        });
                    }}
                >
                    <input
                        type="text"
                        value={name()}
                        onInput={(event) => setName(event.currentTarget.value)}
                    />
                    <button
                        class="primary-button"
                        type="submit"
                        disabled={
                            busy() ||
                            !name().trim() ||
                            name().trim() === props.room.name
                        }
                    >
                        Rename
                    </button>
                </form>
            </Show>

            <Show when={error()}>
                <p class="form-error" role="alert">
                    {error()}
                </p>
            </Show>

            <div class="button-row end">
                <button
                    type="button"
                    class="danger-button"
                    disabled={busy()}
                    onClick={() => void leave()}
                >
                    Leave room
                </button>
            </div>
        </Modal>
    );
}

export function MessageActionsModal(props: {
    client: TolooClient;
    message: Message;
    onClose: () => void;
}): JSX.Element {
    const [error, setError] = createSignal("");
    const [busy, setBusy] = createSignal(false);

    async function run(action: () => Promise<void>): Promise<void> {
        setError("");
        setBusy(true);
        try {
            await action();
            props.onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <Modal
            title="Message"
            subtitle={`${shortKey(props.message.author, 7)} · ${formatTime(props.message.ts)}`}
            onClose={props.onClose}
        >
            <div class="message-quote">{props.message.body}</div>
            <div class="button-row">
                <button
                    type="button"
                    class="ghost-button"
                    disabled={busy()}
                    onClick={() =>
                        void run(async () => {
                            await copyText(props.message.body);
                            props.client.notify("Message copied.", "success");
                        })
                    }
                >
                    <CopyIcon size={15} />
                    Copy text
                </button>
                <button
                    type="button"
                    class="ghost-button"
                    disabled={busy()}
                    onClick={() =>
                        void run(() => props.client.flagMessage(props.message.eid))
                    }
                >
                    Flag as spam
                </button>
                <Show when={!props.message.mine}>
                    <button
                        type="button"
                        class="danger-button"
                        disabled={busy()}
                        onClick={() =>
                            void run(() =>
                                props.client.blockAuthor(props.message.author),
                            )
                        }
                    >
                        Block author
                    </button>
                </Show>
                <Show when={props.message.mine}>
                    <button
                        type="button"
                        class="danger-button"
                        disabled={busy()}
                        onClick={() => {
                            if (!window.confirm("Delete this message for everyone?")) return;
                            void run(() =>
                                props.client.deleteMessage(props.message.eid),
                            );
                        }}
                    >
                        <TrashIcon size={15} />
                        Delete
                    </button>
                </Show>
            </div>
            <Show when={error()}>
                <p class="form-error" role="alert">
                    {error()}
                </p>
            </Show>
            <div class="button-row end">
                <button type="button" class="ghost-button" onClick={props.onClose}>
                    Close
                </button>
            </div>
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

    return (
        <Modal
            title={`Invite to ${props.room.name}`}
            subtitle="Share this verified room.create envelope"
            onClose={props.onClose}
        >
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
            <div class="key-row room-key-row">
                <span>Room key</span>
                <code>{shortKey(props.room.pub, 12)}</code>
            </div>
            <div class="button-row end">
                <button type="button" class="ghost-button" onClick={props.onClose}>
                    Done
                </button>
                <Show when={invite()}>
                    <button
                        type="button"
                        class="primary-button"
                        onClick={() => void copyInvite()}
                    >
                        <CopyIcon size={15} />
                        Copy invite
                    </button>
                </Show>
            </div>
        </Modal>
    );
}
