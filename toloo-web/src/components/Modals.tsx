import {
    For,
    Show,
    createSignal,
    type JSX,
} from "solid-js";
import type { RoomRecord } from "../lib/db";
import type { TolooClient } from "../lib/client";
import {
    allowedProtocols,
    defaultEndpointScheme,
} from "../lib/transport";
import { copyText, downloadText, shortKey } from "../lib/ui";
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
    const [mode, setMode] = createSignal<"create" | "join">("create");
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
