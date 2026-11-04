import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import type { TolooClient } from "../lib/client";
import type { RoomRecord } from "../lib/db";
import { formatTime, shortKey } from "../lib/ui";
import { Avatar } from "./Avatar";
import { InboxIcon, MoreIcon, PlusIcon, RelayIcon } from "./Icons";

export function Sidebar(props: {
    client: TolooClient;
    onAddRoom: () => void;
    onImportRoom: () => void;
    onSettings: () => void;
    onRoomMenu: (room: RoomRecord) => void;
    onOpenChat: () => void;
}): JSX.Element {
    const identity = () => props.client.identity();
    const [query, setQuery] = createSignal("");
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
                </button>
                <span class="sidebar-brand">Toloo</span>
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
                                                {summary()?.unread}
                                            </span>
                                        </Show>
                                    </span>
                                </button>
                                <button
                                    type="button"
                                    class="icon-button subtle room-row-menu"
                                    aria-label={`Actions for ${room.name}`}
                                    onClick={() => props.onRoomMenu(room)}
                                >
                                    <MoreIcon size={16} />
                                </button>
                            </div>
                        );
                    }}
                </For>
            </nav>

            <Show when={props.client.status()}>
                <div class="sidebar-status" title={props.client.status()}>
                    {props.client.status()}
                </div>
            </Show>

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
        </aside>
    );
}
