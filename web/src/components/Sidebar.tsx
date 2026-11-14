import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import type { TolooClient } from "../lib/client";
import type { RoomRecord } from "../lib/db";
import { copyText, formatTime, shortKey } from "../lib/ui";
import { Avatar } from "./Avatar";
import { DropdownMenu, menuPosition, type DropdownEntry } from "./Dropdown";
import {
    CopyIcon,
    EditIcon,
    ExportIcon,
    InboxIcon,
    LogoutIcon,
    MoreIcon,
    PlusIcon,
    RelayIcon,
    ShareIcon,
    SyncIcon,
    TrashIcon,
    UserIcon,
} from "./Icons";

export function Sidebar(props: {
    client: TolooClient;
    onAddRoom: () => void;
    onImportRoom: () => void;
    onNewDm: () => void;
    onSettings: () => void;
    onRenameRoom: (room: RoomRecord) => void;
    onShareRoom: (room: RoomRecord) => void;
    onOpenChat: () => void;
}): JSX.Element {
    const identity = () => props.client.identity();
    const [query, setQuery] = createSignal("");
    const [menu, setMenu] = createSignal<{
        room: RoomRecord;
        x: number;
        y: number;
    } | null>(null);
    const filteredRooms = createMemo(() => {
        const needle = query().trim().toLowerCase();
        if (!needle) return props.client.rooms();
        return props.client.rooms().filter(
            (room) =>
                room.name.toLowerCase().includes(needle) ||
                room.pub.toLowerCase().includes(needle),
        );
    });

    function networkLabel(): string {
        const count = props.client.relays().length;
        if (count === 0) return "No relays";
        return count === 1 ? "1 relay" : `${count} relays`;
    }

    async function copyRoomKey(room: RoomRecord): Promise<void> {
        try {
            await copyText(room.pub);
            props.client.notify("Room key copied.", "success");
        } catch {
            props.client.notify("Could not copy the room key.", "error");
        }
    }

    async function leaveRoom(room: RoomRecord): Promise<void> {
        if (
            !window.confirm(
                "Leave this room? It is removed from this browser; copies on relays are kept.",
            )
        ) {
            return;
        }
        await props.client.leaveRoom(room.pub);
    }

    async function removeRoom(room: RoomRecord): Promise<void> {
        if (
            !window.confirm(
                "Remove this room from this browser? Relay copies are kept.",
            )
        ) {
            return;
        }
        await props.client.removeRoom(room.pub);
    }

    function entries(room: RoomRecord): DropdownEntry[] {
        return [
            {
                label: "Rename room",
                icon: <EditIcon />,
                disabled: !room.roomIdentity,
                onSelect: () => props.onRenameRoom(room),
            },
            {
                label: "Sync now",
                icon: <SyncIcon />,
                onSelect: () => void props.client.syncRoom(room.pub),
            },
            {
                label: "Share invite",
                icon: <ShareIcon />,
                onSelect: () => props.onShareRoom(room),
            },
            {
                label: "Export messages",
                icon: <ExportIcon />,
                onSelect: () => void props.client.exportRoomMessages(room.pub),
            },
            {
                label: "Copy room key",
                icon: <CopyIcon />,
                onSelect: () => void copyRoomKey(room),
            },
            { separator: true },
            {
                label: "Leave room",
                icon: <LogoutIcon />,
                warn: true,
                onSelect: () => void leaveRoom(room),
            },
            {
                label: "Remove from this device",
                icon: <TrashIcon />,
                danger: true,
                onSelect: () => void removeRoom(room),
            },
        ];
    }

    return (
        <aside class="sidebar">
            <header class="sidebar-head">
                <button
                    type="button"
                    class="identity-button"
                    aria-label="Identity and settings"
                    onClick={props.onSettings}
                >
                    <Avatar
                        publicKey={identity()?.sigPub ?? ""}
                        size={36}
                        label="This device"
                    />
                    <span class="sidebar-identity-name">
                        {identity()
                            ? shortKey(identity()!.sigPub, 8)
                            : "No identity"}
                    </span>
                </button>
                <span class="sidebar-spacer" />
                <button
                    type="button"
                    class="icon-button"
                    aria-label="Import"
                    title="Import"
                    onClick={props.onImportRoom}
                >
                    <InboxIcon />
                </button>
                <button
                    type="button"
                    class="icon-button accent"
                    aria-label="New room"
                    title="New room"
                    onClick={props.onAddRoom}
                >
                    <PlusIcon />
                </button>
            </header>

            <div class="sidebar-search">
                <input
                    type="search"
                    placeholder="Search rooms…"
                    aria-label="Search rooms"
                    autocomplete="off"
                    spellcheck={false}
                    value={query()}
                    onInput={(event) => setQuery(event.currentTarget.value)}
                />
            </div>

            <nav class="room-list" aria-label="Rooms">
                <For
                    each={filteredRooms()}
                    fallback={
                        <div class="room-list-empty">
                            <p>
                                No rooms yet.
                                <br />
                                Add a room with +.
                            </p>
                        </div>
                    }
                >
                    {(room) => {
                        const summary = () =>
                            props.client.roomSummaries()[room.pub];
                        return (
                            <div
                                class="room-row"
                                classList={{
                                    active:
                                        !props.client.activeDm() &&
                                        props.client.activePub() === room.pub,
                                }}
                            >
                                <button
                                    type="button"
                                    class="room-row-main"
                                    onClick={() => {
                                        void props.client.selectRoom(room.pub);
                                        props.onOpenChat();
                                    }}
                                >
                                    <Avatar
                                        publicKey={room.pub}
                                        size={40}
                                        label={room.name}
                                    />
                                    <span class="room-row-copy">
                                        <span class="room-row-name">
                                            {room.name}
                                        </span>
                                        <span class="room-row-preview">
                                            {summary()?.preview ?? ""}
                                        </span>
                                    </span>
                                    <span class="room-row-meta">
                                        <span class="room-row-time">
                                            {summary()?.ts
                                                ? formatTime(summary().ts)
                                                : ""}
                                        </span>
                                        <Show
                                            when={
                                                (summary()?.unread ?? 0) > 0
                                            }
                                        >
                                            <span class="room-row-unread">
                                                {(summary()?.unread ?? 0) > 99
                                                    ? "99+"
                                                    : summary()?.unread}
                                            </span>
                                        </Show>
                                    </span>
                                </button>
                                <button
                                    type="button"
                                    class="icon-button subtle room-row-menu"
                                    aria-label={`Actions for ${room.name}`}
                                    onClick={(event) => {
                                        const position = menuPosition(
                                            event.currentTarget,
                                        );
                                        setMenu({ room, ...position });
                                    }}
                                >
                                    <MoreIcon size={16} />
                                </button>
                            </div>
                        );
                    }}
                </For>
            </nav>

            <section class="dm-section">
                <div class="dm-section-head">
                    <span>Direct messages</span>
                    <button
                        type="button"
                        class="icon-button subtle"
                        aria-label="New direct message"
                        title="New direct message"
                        onClick={props.onNewDm}
                    >
                        <UserIcon size={16} />
                    </button>
                </div>
                <div class="dm-list">
                    <Show
                        when={props.client.dmConversations().length > 0}
                        fallback={
                            <p class="dm-empty">No direct messages yet.</p>
                        }
                    >
                        <For each={props.client.dmConversations()}>
                            {(dm) => (
                                <button
                                    type="button"
                                    class="room-row-main dm-row-main"
                                    classList={{
                                        active:
                                            props.client.activeDm() ===
                                            dm.sigPub,
                                    }}
                                    onClick={() => {
                                        void props.client
                                            .selectDm(dm.sigPub)
                                            .then(() => props.onOpenChat());
                                    }}
                                >
                                    <Avatar
                                        publicKey={dm.sigPub}
                                        size={32}
                                        label={dm.name}
                                    />
                                    <span class="room-row-copy">
                                        <span class="room-row-name">
                                            {dm.name}
                                        </span>
                                        <span class="room-row-preview">
                                            {dm.preview ||
                                                (dm.ts
                                                    ? ""
                                                    : "No messages yet")}
                                        </span>
                                    </span>
                                    <span class="room-row-meta">
                                        <span class="room-row-time">
                                            {dm.ts
                                                ? formatTime(dm.ts)
                                                : ""}
                                        </span>
                                        <Show when={dm.unread > 0}>
                                            <span class="room-row-unread">
                                                {dm.unread > 99
                                                    ? "99+"
                                                    : dm.unread}
                                            </span>
                                        </Show>
                                    </span>
                                </button>
                            )}
                        </For>
                    </Show>
                </div>
            </section>

            <footer class="sidebar-foot">
                <span class="sidebar-network">{networkLabel()}</span>
                <span class="sidebar-spacer" />
                <button
                    type="button"
                    class="icon-button"
                    aria-label="Relays"
                    title="Relays"
                    onClick={props.onSettings}
                >
                    <RelayIcon />
                </button>
            </footer>

            <Show when={menu()}>
                {(state) => (
                    <DropdownMenu
                        x={state().x}
                        y={state().y}
                        entries={entries(state().room)}
                        onClose={() => setMenu(null)}
                    />
                )}
            </Show>
        </aside>
    );
}
