import {
    For,
    Show,
    createEffect,
    createSignal,
    type JSX,
} from "solid-js";
import type { TolooClient } from "../lib/client";
import { formatDay, formatTime, sameDay, shortKey } from "../lib/ui";
import { Avatar } from "./Avatar";
import {
    ArrowLeftIcon,
    LockIcon,
    SendIcon,
    SyncIcon,
} from "./Icons";

export function DmThread(props: {
    client: TolooClient;
    peer: string;
    onBack: () => void;
}): JSX.Element {
    const [draft, setDraft] = createSignal("");
    const [sending, setSending] = createSignal(false);
    const [syncing, setSyncing] = createSignal(false);
    let scrollArea: HTMLDivElement | undefined;

    const contact = () =>
        props.client.activeDmContact() ??
        props.client.dmConversations().find((item) => item.sigPub === props.peer);
    const name = () => contact()?.name ?? shortKey(props.peer, 8);

    async function sync(): Promise<void> {
        if (syncing()) return;
        setSyncing(true);
        try {
            await props.client.syncDms();
        } finally {
            setSyncing(false);
        }
    }

    async function send(event?: SubmitEvent): Promise<void> {
        event?.preventDefault();
        const body = draft().trim();
        if (!body || sending()) return;
        setSending(true);
        try {
            await props.client.sendDm(props.peer, body);
            setDraft("");
        } catch (error) {
            props.client.notify(
                error instanceof Error ? error.message : String(error),
                "error",
            );
        } finally {
            setSending(false);
        }
    }

    createEffect(() => {
        props.client.activeDmMessages();
        queueMicrotask(() => {
            if (scrollArea) scrollArea.scrollTop = scrollArea.scrollHeight;
        });
    });

    return (
        <section class="chat">
            <header class="chat-head">
                <button
                    type="button"
                    class="icon-button mobile-only chat-back"
                    aria-label="Back to rooms"
                    onClick={props.onBack}
                >
                    <ArrowLeftIcon />
                </button>
                <Avatar publicKey={props.peer} size={34} label={name()} />
                <div class="chat-title">
                    <h1>{name()}</h1>
                    <div class="chat-subline dm-subline">
                        <LockIcon size={12} />
                        <span>End-to-end encrypted</span>
                        <span>·</span>
                        <span>{shortKey(props.peer, 8)}</span>
                    </div>
                </div>
                <div class="chat-actions">
                    <button
                        type="button"
                        class="icon-button action-sync"
                        classList={{ spinning: syncing() }}
                        aria-label="Sync direct messages"
                        title="Sync direct messages"
                        disabled={syncing()}
                        onClick={() => void sync()}
                    >
                        <SyncIcon />
                    </button>
                </div>
            </header>

            <div class="message-scroll" ref={scrollArea}>
                <Show
                    when={props.client.activeDmMessages().length > 0}
                    fallback={
                        <div class="chat-empty empty-messages">
                            <LockIcon size={28} />
                            <p>
                                No messages yet.
                                <br />
                                Private messages are encrypted for this device.
                            </p>
                        </div>
                    }
                >
                    <For each={props.client.activeDmMessages()}>
                        {(message, index) => {
                            const previous = () =>
                                props.client.activeDmMessages()[index() - 1];
                            const newDay = () =>
                                !previous() ||
                                !sameDay(previous().ts, message.ts);
                            const grouped = () =>
                                !newDay() &&
                                previous()?.mine === message.mine;
                            return (
                                <article
                                    class="message"
                                    classList={{
                                        mine: message.mine,
                                        grouped: grouped(),
                                    }}
                                >
                                    <Show when={!message.mine}>
                                        <span class="message-gutter">
                                            <Show when={!grouped()}>
                                                <Avatar
                                                    publicKey={message.peer}
                                                    size={26}
                                                    label={shortKey(
                                                        message.peer,
                                                        8,
                                                    )}
                                                />
                                            </Show>
                                        </span>
                                    </Show>
                                    <div class="message-stack">
                                        <div class="bubble">
                                            <span class="bubble-text">
                                                {message.body}
                                            </span>
                                            <div class="bubble-meta">
                                                <span class="bubble-clock">
                                                    {newDay()
                                                        ? `${formatDay(message.ts)} · ${formatTime(message.ts)}`
                                                        : formatTime(message.ts)}
                                                </span>
                                                <Show when={message.mine}>
                                                    <span
                                                        class={`bubble-status status-${message.depth >= 2 ? "sent" : "sending"}`}
                                                    >
                                                        {message.depth >= 2
                                                            ? "delivered"
                                                            : "sending"}
                                                    </span>
                                                </Show>
                                            </div>
                                        </div>
                                    </div>
                                </article>
                            );
                        }}
                    </For>
                </Show>
            </div>

            <form class="composer" onSubmit={(event) => void send(event)}>
                <span
                    class="composer-lock"
                    title="End-to-end encrypted"
                    aria-hidden="true"
                >
                    <LockIcon size={16} />
                </span>
                <input
                    type="text"
                    placeholder="Encrypted message…"
                    aria-label="Private message"
                    value={draft()}
                    onInput={(event) =>
                        setDraft(event.currentTarget.value)
                    }
                />
                <button
                    class="send-button"
                    type="submit"
                    aria-label="Send private message"
                    disabled={sending() || !draft().trim()}
                >
                    <SendIcon />
                </button>
            </form>
        </section>
    );
}
