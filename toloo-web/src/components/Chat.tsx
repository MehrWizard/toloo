import {
    For,
    Show,
    createEffect,
    createSignal,
    type JSX,
} from "solid-js";
import type { Message, TolooClient } from "../lib/client";
import {
    avatarHue,
    copyText,
    formatDay,
    formatTime,
    sameDay,
    shortKey,
} from "../lib/ui";
import {
    CheckIcon,
    ClockIcon,
    MoreIcon,
    PeopleIcon,
    SendIcon,
    SettingsIcon,
    ShieldIcon,
    SyncIcon,
} from "./Icons";
import { MessageActionsModal } from "./Modals";

export function Chat(props: {
    client: TolooClient;
    onMenu: () => void;
    onInvite: () => void;
    onRoomMenu: () => void;
    onSettings: () => void;
}): JSX.Element {
    const [draft, setDraft] = createSignal("");
    const [sending, setSending] = createSignal(false);
    const [selectedMessage, setSelectedMessage] = createSignal<Message | null>(null);
    let scrollArea: HTMLDivElement | undefined;
    let composer: HTMLTextAreaElement | undefined;

    const room = () => props.client.activeRoom();

    createEffect(() => {
        const count = props.client.messages().length;
        const pub = props.client.activePub();
        if (!count && !pub) return;
        queueMicrotask(() => {
            if (scrollArea) scrollArea.scrollTop = scrollArea.scrollHeight;
        });
    });

    async function send(event?: SubmitEvent): Promise<void> {
        event?.preventDefault();
        const body = draft().trim();
        if (!body || sending()) return;
        setSending(true);
        try {
            await props.client.sendMessage(body);
            setDraft("");
            if (composer) {
                composer.style.height = "auto";
                composer.focus();
            }
        } catch {
            // The client raises a toast for relay failures.
        } finally {
            setSending(false);
        }
    }

    function onComposerKeyDown(event: KeyboardEvent): void {
        if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void send();
        }
    }

    function resizeComposer(): void {
        if (!composer) return;
        composer.style.height = "auto";
        composer.style.height = `${Math.min(composer.scrollHeight, 160)}px`;
    }

    return (
        <Show when={room()} fallback={<EmptyChat />}>
            {(current) => (
                <section class="chat">
                    <header class="chat-head">
                        <button
                            type="button"
                            class="icon-button mobile-only"
                            aria-label="Open rooms"
                            onClick={props.onMenu}
                        >
                            <span class="menu-glyph" aria-hidden="true">
                                ☰
                            </span>
                        </button>
                        <span
                            class="room-avatar large"
                            style={{ "--hue": `${avatarHue(current().pub)}` }}
                        >
                            {current().name.slice(0, 1).toUpperCase()}
                        </span>
                        <div class="chat-title">
                            <h1>{current().name}</h1>
                            <div class="chat-subline">
                                <button
                                    type="button"
                                    class="inline-key"
                                    title="Copy room public key"
                                    onClick={() => {
                                        void copyText(current().pub)
                                            .then(() =>
                                                props.client.notify(
                                                    "Room key copied.",
                                                    "success",
                                                ),
                                            )
                                            .catch(() =>
                                                props.client.notify(
                                                    "Could not copy the room key.",
                                                    "error",
                                                ),
                                            );
                                    }}
                                >
                                    {shortKey(current().pub, 8)}
                                </button>
                                <Show when={props.client.members().length > 0}>
                                    <span class="meta-dot" aria-hidden="true" />
                                    <span class="member-count">
                                        <PeopleIcon size={13} />
                                        {props.client.members().length}{" "}
                                        {props.client.members().length === 1
                                            ? "member"
                                            : "members"}
                                    </span>
                                </Show>
                                <Show when={current().roomIdentity}>
                                    <span class="meta-dot" aria-hidden="true" />
                                    <span class="room-authority">
                                        <ShieldIcon size={13} />
                                        room key
                                    </span>
                                </Show>
                            </div>
                        </div>
                        <div class="chat-actions">
                            <button
                                type="button"
                                class="ghost-button compact"
                                onClick={props.onInvite}
                            >
                                Invite
                            </button>
                            <button
                                type="button"
                                class="icon-button"
                                classList={{ spinning: props.client.syncing() }}
                                aria-label="Sync room"
                                title="Sync room"
                                disabled={props.client.syncing()}
                                onClick={() => void props.client.syncRoom()}
                            >
                                <SyncIcon />
                            </button>
                            <button
                                type="button"
                                class="icon-button"
                                aria-label="Room actions"
                                title="Room actions"
                                onClick={props.onRoomMenu}
                            >
                                <MoreIcon />
                            </button>
                            <button
                                type="button"
                                class="icon-button"
                                aria-label="Settings"
                                title="Settings"
                                onClick={props.onSettings}
                            >
                                <SettingsIcon />
                            </button>
                        </div>
                    </header>

                    <Show when={props.client.relays().length === 0}>
                        <div class="banner">
                            <span>
                                This room is local only. Add a relay to publish
                                and receive envelopes.
                            </span>
                            <button
                                type="button"
                                class="text-button"
                                onClick={props.onSettings}
                            >
                                Add relay
                            </button>
                        </div>
                    </Show>
                    <Show when={props.client.syncState() === "error"}>
                        <div class="banner banner-error">
                            <span>Last sync failed. Check the relay in Settings.</span>
                            <button
                                type="button"
                                class="text-button"
                                onClick={() => void props.client.syncRoom()}
                            >
                                Retry
                            </button>
                        </div>
                    </Show>

                    <div class="message-scroll" ref={scrollArea}>
                        <Show
                            when={props.client.messages().length > 0}
                            fallback={
                                <div class="chat-empty">
                                    <span class="empty-mark" aria-hidden="true">
                                        ↯
                                    </span>
                                    <h2>No messages yet</h2>
                                    <p>
                                        Write the first envelope. It is signed on
                                        this device and stored by relays.
                                    </p>
                                </div>
                            }
                        >
                            <For each={props.client.messages()}>
                                {(message, index) => {
                                    const previous = () =>
                                        props.client.messages()[index() - 1];
                                    const newDay = () =>
                                        !previous() ||
                                        !sameDay(previous().ts, message.ts);
                                    const grouped = () =>
                                        !newDay() &&
                                        previous()?.author === message.author;
                                    return (
                                        <>
                                            <Show when={newDay()}>
                                                <div class="day-divider">
                                                    <span>{formatDay(message.ts)}</span>
                                                </div>
                                            </Show>
                                            <article
                                                class="message"
                                                classList={{
                                                    mine: message.mine,
                                                    grouped: grouped(),
                                                }}
                                            >
                                                <Show when={!message.mine}>
                                                    <span
                                                        class="message-avatar"
                                                        style={{
                                                            "--hue": `${avatarHue(message.author)}`,
                                                        }}
                                                    >
                                                        {message.author
                                                            .slice(0, 1)
                                                            .toUpperCase()}
                                                    </span>
                                                </Show>
                                                <div class="message-body">
                                                    <Show when={!grouped() && !message.mine}>
                                                        <span class="message-author">
                                                            {shortKey(message.author, 6)}
                                                        </span>
                                                    </Show>
                                                    <div class="bubble">
                                                        <span class="bubble-text">
                                                            {message.body}
                                                        </span>
                                                    </div>
                                                    <div class="message-meta">
                                                        <time
                                                            dateTime={new Date(
                                                                message.ts,
                                                            ).toISOString()}
                                                        >
                                                            {formatTime(message.ts)}
                                                        </time>
                                                        <Show when={message.mine}>
                                                            <span
                                                                class="delivery"
                                                                title={
                                                                    message.depth >= 2
                                                                        ? "Committed"
                                                                        : "Waiting for a commit"
                                                                }
                                                            >
                                                                {message.depth >= 2 ? (
                                                                    <CheckIcon size={13} />
                                                                ) : (
                                                                    <ClockIcon size={13} />
                                                                )}
                                                                {message.depth >= 2
                                                                    ? "sent"
                                                                    : "pending"}
                                                            </span>
                                                        </Show>
                                                        <button
                                                            type="button"
                                                            class="icon-button subtle message-more"
                                                            aria-label="Message actions"
                                                            onClick={() =>
                                                                setSelectedMessage(message)
                                                            }
                                                        >
                                                            <MoreIcon size={14} />
                                                        </button>
                                                    </div>
                                                </div>
                                            </article>
                                        </>
                                    );
                                }}
                            </For>
                        </Show>
                    </div>

                    <form class="composer" onSubmit={(event) => void send(event)}>
                        <textarea
                            ref={composer}
                            rows={1}
                            placeholder={`Message #${current().name.toLowerCase()}`}
                            aria-label="Message"
                            value={draft()}
                            onInput={(event) => {
                                setDraft(event.currentTarget.value);
                                resizeComposer();
                            }}
                            onKeyDown={onComposerKeyDown}
                        />
                        <button
                            class="send-button"
                            type="submit"
                            aria-label="Send message"
                            disabled={sending() || draft().trim().length === 0}
                        >
                            <SendIcon size={19} />
                        </button>
                    </form>
                    <p class="composer-hint">
                        Envelopes are signed locally. Enter to send, Shift+Enter
                        for a new line.
                    </p>
                    <Show when={selectedMessage()}>
                        {(message) => (
                            <MessageActionsModal
                                client={props.client}
                                message={message()}
                                onClose={() => setSelectedMessage(null)}
                            />
                        )}
                    </Show>
                </section>
            )}
        </Show>
    );
}

function EmptyChat(): JSX.Element {
    return (
        <section class="chat empty-chat">
            <div class="empty-chat-inner">
                <div class="empty-orbit" aria-hidden="true">
                    <span />
                </div>
                <h1>Pick a room, or make one.</h1>
                <p>
                    Rooms are keypairs. Messages are envelopes. Relays pass them
                    along when they fit.
                </p>
            </div>
        </section>
    );
}
