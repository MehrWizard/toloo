import {
    For,
    Show,
    createEffect,
    createSignal,
    onCleanup,
    onMount,
    type JSX,
} from "solid-js";
import type { Message, TolooClient } from "../lib/client";
import type { RoomRecord } from "../lib/db";
import { copyText, formatDay, formatTime, sameDay, shortKey } from "../lib/ui";
import { Avatar } from "./Avatar";
import { DropdownMenu, EmojiPicker, menuPosition, type DropdownEntry } from "./Dropdown";
import {
    ArrowLeftIcon,
    BlockIcon,
    CheckIcon,
    CloseIcon,
    CopyIcon,
    EditIcon,
    EmojiIcon,
    ExportIcon,
    FlagIcon,
    LogoutIcon,
    MoreIcon,
    SendIcon,
    SettingsIcon,
    ShareIcon,
    SyncIcon,
    TrashIcon,
    UserIcon,
} from "./Icons";

export function Chat(props: {
    client: TolooClient;
    onBack: () => void;
    onInvite: () => void;
    onRenameRoom: (room: RoomRecord) => void;
    onSettings: () => void;
}): JSX.Element {
    const [draft, setDraft] = createSignal("");
    const [sending, setSending] = createSignal(false);
    const [roomMenu, setRoomMenu] = createSignal<{
        room: RoomRecord;
        x: number;
        y: number;
    } | null>(null);
    const [messageMenu, setMessageMenu] = createSignal<{
        message: Message;
        x: number;
        y: number;
    } | null>(null);
    const [emojiMenu, setEmojiMenu] = createSignal<{
        x: number;
        y: number;
    } | null>(null);
    const [selected, setSelected] = createSignal<string[]>([]);
    let scrollArea: HTMLDivElement | undefined;
    let longPressTimer: number | undefined;
    let longPressFired = false;
    let dragSelecting = false;
    let dragLastEid = "";
    let dragClientX = 0;
    let dragClientY = 0;
    let autoScrollDirection = 0;
    let autoScrollFrame: number | undefined;

    const room = () => props.client.activeRoom();

    function subline(pub: string): string {
        const key = shortKey(pub, 8);
        const count = props.client.members().length;
        return `${key} · ${count} ${count === 1 ? "member" : "members"}`;
    }

    function run(action: () => Promise<void>): void {
        action().catch((error) => {
            props.client.notify(
                error instanceof Error ? error.message : String(error),
                "error",
            );
        });
    }

    async function copyRoomKey(roomRecord: RoomRecord): Promise<void> {
        try {
            await copyText(roomRecord.pub);
            props.client.notify("Room key copied.", "success");
        } catch {
            props.client.notify("Could not copy the room key.", "error");
        }
    }

    async function copyMessage(message: Message): Promise<void> {
        try {
            await copyText(message.body);
            props.client.notify("Message copied.", "success");
        } catch {
            props.client.notify("Could not copy the message.", "error");
        }
    }

    async function leaveRoom(roomRecord: RoomRecord): Promise<void> {
        if (
            !window.confirm(
                "Leave this room? It is removed from this browser; copies on relays are kept.",
            )
        ) {
            return;
        }
        await props.client.leaveRoom(roomRecord.pub);
    }

    async function removeRoom(roomRecord: RoomRecord): Promise<void> {
        if (
            !window.confirm(
                "Remove this room from this browser? Relay copies are kept.",
            )
        ) {
            return;
        }
        await props.client.removeRoom(roomRecord.pub);
    }

    function roomEntries(roomRecord: RoomRecord): DropdownEntry[] {
        return [
            {
                label: "Rename room",
                icon: <EditIcon />,
                disabled: !roomRecord.roomIdentity,
                onSelect: () => props.onRenameRoom(roomRecord),
            },
            {
                label: "Sync now",
                icon: <SyncIcon />,
                onSelect: () =>
                    run(() => props.client.syncRoom(roomRecord.pub)),
            },
            {
                label: "Share invite",
                icon: <ShareIcon />,
                onSelect: props.onInvite,
            },
            {
                label: "Export messages",
                icon: <ExportIcon />,
                onSelect: () =>
                    run(() =>
                        props.client.exportRoomMessages(roomRecord.pub),
                    ),
            },
            {
                label: "Copy room key",
                icon: <CopyIcon />,
                onSelect: () => void copyRoomKey(roomRecord),
            },
            { separator: true },
            {
                label: "Leave room",
                icon: <LogoutIcon />,
                warn: true,
                onSelect: () => run(() => leaveRoom(roomRecord)),
            },
            {
                label: "Remove from this device",
                icon: <TrashIcon />,
                danger: true,
                onSelect: () => run(() => removeRoom(roomRecord)),
            },
        ];
    }

    function messageEntries(message: Message): DropdownEntry[] {
        const entries: DropdownEntry[] = [
            {
                label: "Copy message text",
                icon: <CopyIcon />,
                onSelect: () => void copyMessage(message),
            },
            {
                label: "Flag as spam",
                icon: <FlagIcon />,
                warn: true,
                onSelect: () =>
                    run(() => props.client.flagMessage(message.eid)),
            },
            {
                label: "Block author",
                icon: <BlockIcon />,
                danger: true,
                onSelect: () =>
                    run(() => props.client.blockAuthor(message.author)),
            },
            {
                label: "Attest +",
                icon: <CheckIcon />,
                success: true,
                onSelect: () =>
                    run(() =>
                        props.client.attestAuthor(
                            message.author,
                            "positive",
                        ),
                    ),
            },
            {
                label: "Attest −",
                icon: <CloseIcon />,
                danger: true,
                onSelect: () =>
                    run(() =>
                        props.client.attestAuthor(
                            message.author,
                            "negative",
                        ),
                    ),
            },
        ];
        if (message.mine) {
            entries.push(
                { separator: true },
                {
                    label: "Delete message",
                    icon: <TrashIcon />,
                    danger: true,
                    onSelect: () => {
                        if (
                            !window.confirm(
                                "Delete this message for everyone?",
                            )
                        ) {
                            return;
                        }
                        run(() =>
                            props.client.deleteMessage(message.eid),
                        );
                    },
                },
            );
        }
        return entries;
    }

    function toggleSelected(eid: string): void {
        setSelected((current) =>
            current.includes(eid)
                ? current.filter((item) => item !== eid)
                : [...current, eid],
        );
    }

    function hitTestSelection(): void {
        if (!dragSelecting) return;
        const element = document
            .elementFromPoint(dragClientX, dragClientY)
            ?.closest("[data-message-eid]") as HTMLElement | null;
        const eid = element?.dataset.messageEid;
        if (eid && eid !== dragLastEid) {
            dragLastEid = eid;
            toggleSelected(eid);
        }
    }

    function autoScrollLoop(): void {
        if (!dragSelecting || autoScrollDirection === 0 || !scrollArea) {
            autoScrollFrame = undefined;
            return;
        }
        scrollArea.scrollTop += autoScrollDirection * 10;
        hitTestSelection();
        autoScrollFrame = requestAnimationFrame(autoScrollLoop);
    }

    function startDragSelection(eid: string): void {
        dragSelecting = true;
        dragLastEid = eid;
        setSelected((current) =>
            current.includes(eid) ? current : [...current, eid],
        );
    }

    function onDocumentPointerMove(event: PointerEvent): void {
        if (!dragSelecting || !scrollArea) return;
        dragClientX = event.clientX;
        dragClientY = event.clientY;
        hitTestSelection();
        const rect = scrollArea.getBoundingClientRect();
        const next =
            event.clientY < rect.top + 60
                ? -1
                : event.clientY > rect.bottom - 60
                  ? 1
                  : 0;
        autoScrollDirection = next;
        if (next !== 0 && autoScrollFrame === undefined) {
            autoScrollFrame = requestAnimationFrame(autoScrollLoop);
        }
    }

    function onDocumentPointerUp(): void {
        dragSelecting = false;
        autoScrollDirection = 0;
        if (longPressTimer !== undefined) {
            window.clearTimeout(longPressTimer);
            longPressTimer = undefined;
        }
    }

    function startGutterPress(event: PointerEvent, message: Message): void {
        if (event.button !== 0) return;
        longPressFired = false;
        dragLastEid = "";
        dragClientX = event.clientX;
        dragClientY = event.clientY;
        if (longPressTimer !== undefined) window.clearTimeout(longPressTimer);
        longPressTimer = window.setTimeout(() => {
            longPressFired = true;
            startDragSelection(message.eid);
        }, 450);
    }

    function endGutterPress(message: Message): void {
        if (longPressTimer !== undefined) {
            window.clearTimeout(longPressTimer);
            longPressTimer = undefined;
        }
        if (!longPressFired && selected().length > 0) {
            toggleSelected(message.eid);
        }
        longPressFired = false;
        onDocumentPointerUp();
    }

    function clearSelection(): void {
        setSelected([]);
        onDocumentPointerUp();
    }

    async function copySelected(): Promise<void> {
        const text = props.client
            .messages()
            .filter((message) => selected().includes(message.eid))
            .map((message) => message.body)
            .join("\n");
        if (!text) return;
        try {
            await copyText(text);
            props.client.notify("Selected messages copied.", "success");
        } catch {
            props.client.notify("Could not copy the selected messages.", "error");
        }
        clearSelection();
    }

    async function deleteSelected(): Promise<void> {
        if (
            !window.confirm(
                `Delete ${selected().length} selected message(s)?`,
            )
        ) {
            return;
        }
        const eids = selected();
        clearSelection();
        for (const eid of eids) {
            try {
                await props.client.deleteMessage(eid);
            } catch (error) {
                props.client.notify(
                    error instanceof Error ? error.message : String(error),
                    "error",
                );
                break;
            }
        }
    }

    onMount(() => {
        document.addEventListener("pointermove", onDocumentPointerMove, true);
        document.addEventListener("pointerup", onDocumentPointerUp, true);
        document.addEventListener("pointercancel", onDocumentPointerUp, true);
        onCleanup(() => {
            document.removeEventListener(
                "pointermove",
                onDocumentPointerMove,
                true,
            );
            document.removeEventListener(
                "pointerup",
                onDocumentPointerUp,
                true,
            );
            document.removeEventListener(
                "pointercancel",
                onDocumentPointerUp,
                true,
            );
            if (autoScrollFrame !== undefined) cancelAnimationFrame(autoScrollFrame);
        });
    });

    createEffect(() => {
        props.client.activePub();
        clearSelection();
    });

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
        } catch {
            // The client reports relay failures through the status bar.
        } finally {
            setSending(false);
        }
    }

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
                <Show
                    when={room()}
                    fallback={
                        <h1 class="chat-select-title">Select a room</h1>
                    }
                >
                    {(current) => (
                        <>
                            <Avatar
                                publicKey={current().pub}
                                size={34}
                                label={current().name}
                            />
                            <div class="chat-title">
                                <h1>{current().name}</h1>
                                <div class="chat-subline">
                                    {subline(current().pub)}
                                </div>
                            </div>
                        </>
                    )}
                </Show>
                <Show when={room()}>
                    {(current) => (
                        <div class="chat-actions">
                            <button
                                type="button"
                                class="icon-button action-sync"
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
                                class="icon-button action-share"
                                aria-label="Share invite"
                                title="Share invite"
                                onClick={props.onInvite}
                            >
                                <ShareIcon />
                            </button>
                            <button
                                type="button"
                                class="icon-button action-export"
                                aria-label="Export messages"
                                title="Export messages"
                                onClick={() =>
                                    void props.client.exportRoomMessages(
                                        current().pub,
                                    )
                                }
                            >
                                <ExportIcon />
                            </button>
                            <button
                                type="button"
                                class="icon-button"
                                aria-label="Room actions"
                                title="Room actions"
                                onClick={(event) => {
                                    const position = menuPosition(
                                        event.currentTarget,
                                    );
                                    setRoomMenu({
                                        room: current(),
                                        ...position,
                                    });
                                }}
                            >
                                <MoreIcon />
                            </button>
                        </div>
                    )}
                </Show>
                <button
                    type="button"
                    class="icon-button"
                    aria-label="Settings"
                    title="Settings"
                    onClick={props.onSettings}
                >
                    <SettingsIcon />
                </button>
            </header>
            <Show when={selected().length > 0}>
                <header class="chat-head selection-head">
                    <button
                        type="button"
                        class="icon-button"
                        aria-label="Cancel selection"
                        onClick={clearSelection}
                    >
                        <CloseIcon />
                    </button>
                    <span class="selection-count">
                        {selected().length} selected
                    </span>
                    <span class="chat-spacer" />
                    <button
                        type="button"
                        class="icon-button"
                        aria-label="Copy selected messages"
                        onClick={() => void copySelected()}
                    >
                        <CopyIcon />
                    </button>
                    <button
                        type="button"
                        class="icon-button tint-danger"
                        aria-label="Delete selected messages"
                        onClick={() => void deleteSelected()}
                    >
                        <TrashIcon />
                    </button>
                </header>
            </Show>

            <Show
                when={room()}
                fallback={
                    <div class="chat-empty empty-room">
                        <UserIcon size={48} class="empty-room-icon" />
                        <p>Select a room to start chatting</p>
                    </div>
                }
            >
                {(current) => (
                    <>
                        <div class="message-scroll" ref={scrollArea}>
                            <Show
                                when={props.client.messages().length > 0}
                                fallback={
                                    <div class="chat-empty empty-messages">
                                        <p>
                                            No messages yet.
                                            <br />
                                            Say hello.
                                        </p>
                                    </div>
                                }
                            >
                                <For each={props.client.messages()}>
                                    {(message, index) => {
                                        const previous = () =>
                                            props.client.messages()[
                                                index() - 1
                                            ];
                                        const newDay = () =>
                                            !previous() ||
                                            !sameDay(
                                                previous().ts,
                                                message.ts,
                                            );
                                        const grouped = () =>
                                            !newDay() &&
                                            previous()?.author ===
                                                message.author;
                                        const status = () =>
                                            message.depth >= 2
                                                ? "sent"
                                                : "sending";
                                        return (
                                            <article
                                                class="message"
                                                data-message-eid={message.eid}
                                                classList={{
                                                    mine: message.mine,
                                                    grouped: grouped(),
                                                    selected: selected().includes(message.eid),
                                                }}
                                            >
                                                <Show when={!message.mine}>
                                                    <span
                                                        class="message-gutter"
                                                        onPointerDown={(event) =>
                                                            startGutterPress(
                                                                event,
                                                                message,
                                                            )
                                                        }
                                                        onPointerUp={() =>
                                                            endGutterPress(
                                                                message,
                                                            )
                                                        }
                                                        onPointerCancel={
                                                            onDocumentPointerUp
                                                        }
                                                    >
                                                        <Show when={!grouped()}>
                                                            <Avatar
                                                                publicKey={
                                                                    message.author
                                                                }
                                                                size={26}
                                                                label={shortKey(
                                                                    message.author,
                                                                    8,
                                                                )}
                                                            />
                                                        </Show>
                                                        <Show
                                                            when={
                                                                selected()
                                                                    .length > 0
                                                            }
                                                        >
                                                            <span
                                                                class="selection-check"
                                                                classList={{
                                                                    selected:
                                                                        selected().includes(
                                                                            message.eid,
                                                                        ),
                                                                }}
                                                            >
                                                                <Show
                                                                    when={selected().includes(
                                                                        message.eid,
                                                                    )}
                                                                >
                                                                    <CheckIcon size={12} />
                                                                </Show>
                                                            </span>
                                                        </Show>
                                                    </span>
                                                </Show>
                                                <div class="message-stack">
                                                    <Show
                                                        when={
                                                            !grouped() &&
                                                            !message.mine
                                                        }
                                                    >
                                                        <span class="message-author">
                                                            {shortKey(
                                                                message.author,
                                                                8,
                                                            )}
                                                        </span>
                                                    </Show>
                                                    <div class="bubble">
                                                        <span class="bubble-text">
                                                            {message.body}
                                                        </span>
                                                        <div class="bubble-meta">
                                                            <span class="bubble-clock">
                                                                {newDay()
                                                                    ? `${formatDay(message.ts)} · ${formatTime(message.ts)}`
                                                                    : formatTime(
                                                                          message.ts,
                                                                      )}
                                                            </span>
                                                            <Show
                                                                when={
                                                                    message.mine
                                                                }
                                                            >
                                                                <span
                                                                    class={`bubble-status status-${status()}`}
                                                                >
                                                                    {status()}
                                                                </span>
                                                            </Show>
                                                            <button
                                                                type="button"
                                                                class="icon-button subtle message-more"
                                                                aria-label="Message actions"
                                                                onClick={(
                                                                    event,
                                                                ) => {
                                                                    const position =
                                                                        menuPosition(
                                                                            event.currentTarget,
                                                                        );
                                                                    setMessageMenu(
                                                                        {
                                                                            message,
                                                                            ...position,
                                                                        },
                                                                    );
                                                                }}
                                                            >
                                                                <MoreIcon
                                                                    size={14}
                                                                />
                                                            </button>
                                                        </div>
                                                    </div>
                                                </div>
                                                <Show when={message.mine}>
                                                    <span
                                                        class="message-gutter"
                                                        onPointerDown={(event) =>
                                                            startGutterPress(
                                                                event,
                                                                message,
                                                            )
                                                        }
                                                        onPointerUp={() =>
                                                            endGutterPress(
                                                                message,
                                                            )
                                                        }
                                                        onPointerCancel={
                                                            onDocumentPointerUp
                                                        }
                                                    >
                                                        <Show when={!grouped()}>
                                                            <Avatar
                                                                publicKey={
                                                                    message.author
                                                                }
                                                                size={26}
                                                                label="You"
                                                            />
                                                        </Show>
                                                        <Show
                                                            when={
                                                                selected()
                                                                    .length > 0
                                                            }
                                                        >
                                                            <span
                                                                class="selection-check"
                                                                classList={{
                                                                    selected:
                                                                        selected().includes(
                                                                            message.eid,
                                                                        ),
                                                                }}
                                                            >
                                                                <Show
                                                                    when={selected().includes(
                                                                        message.eid,
                                                                    )}
                                                                >
                                                                    <CheckIcon size={12} />
                                                                </Show>
                                                            </span>
                                                        </Show>
                                                    </span>
                                                </Show>
                                            </article>
                                        );
                                    }}
                                </For>
                            </Show>
                        </div>

                        <form
                            class="composer"
                            onSubmit={(event) => void send(event)}
                        >
                            <button
                                type="button"
                                class="icon-button composer-emoji"
                                aria-label="Emoji"
                                title="Emoji"
                                onClick={(event) => {
                                    const rect = event.currentTarget.getBoundingClientRect();
                                    setEmojiMenu({
                                        x: rect.left,
                                        y: rect.top - 190,
                                    });
                                }}
                            >
                                <EmojiIcon />
                            </button>
                            <input
                                type="text"
                                placeholder="Message…"
                                aria-label="Message"
                                value={draft()}
                                onInput={(event) =>
                                    setDraft(event.currentTarget.value)
                                }
                            />
                            <button
                                class="send-button"
                                type="submit"
                                aria-label="Send message"
                                disabled={
                                    sending() ||
                                    draft().trim().length === 0
                                }
                            >
                                <SendIcon size={19} />
                            </button>
                        </form>
                    </>
                )}
            </Show>

            <Show when={roomMenu()}>
                {(state) => (
                    <DropdownMenu
                        x={state().x}
                        y={state().y}
                        entries={roomEntries(state().room)}
                        onClose={() => setRoomMenu(null)}
                    />
                )}
            </Show>
            <Show when={messageMenu()}>
                {(state) => (
                    <DropdownMenu
                        x={state().x}
                        y={state().y}
                        entries={messageEntries(state().message)}
                        onClose={() => setMessageMenu(null)}
                    />
                )}
            </Show>
            <Show when={emojiMenu()}>
                {(state) => (
                    <EmojiPicker
                        x={state().x}
                        y={state().y}
                        onPick={(emoji) => setDraft((value) => value + emoji)}
                        onClose={() => setEmojiMenu(null)}
                    />
                )}
            </Show>
        </section>
    );
}
