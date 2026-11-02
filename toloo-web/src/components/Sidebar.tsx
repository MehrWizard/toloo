import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import type { TolooClient } from "../lib/client";
import { avatarHue, formatTime, shortKey } from "../lib/ui";
import { PlusIcon, SettingsIcon, ShieldIcon } from "./Icons";

export function Sidebar(props: {
    client: TolooClient;
    open: boolean;
    onClose: () => void;
    onAddRoom: () => void;
    onSettings: () => void;
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

    function statusLabel(): string {
        if (props.client.relays().length === 0) return "No relay";
        const state = props.client.syncState();
        if (state === "syncing") return "Syncing";
        if (state === "error") return "Relay error";
        if (state === "offline") return "Offline";
        return "Connected";
    }

    return (
        <>
            <Show when={props.open}>
                <button
                    class="sidebar-scrim"
                    type="button"
                    aria-label="Close rooms"
                    onClick={props.onClose}
                />
            </Show>
            <aside class="sidebar" classList={{ open: props.open }}>
                <header class="sidebar-head">
                    <div class="wordmark small">
                        <span class="wordmark-mark" aria-hidden="true">
                            T
                        </span>
                        <span>toloo</span>
                    </div>
                    <div class="connection-pill" data-state={props.client.syncState()}>
                        <span class="status-dot" aria-hidden="true" />
                        <span>{statusLabel()}</span>
                    </div>
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

                <div class="sidebar-section-head">
                    <span>Rooms</span>
                    <button
                        type="button"
                        class="text-button"
                        onClick={props.onAddRoom}
                    >
                        <PlusIcon size={15} />
                        Add
                    </button>
                </div>

                <nav class="room-list" aria-label="Rooms">
                    <Show
                        when={filteredRooms().length > 0}
                        fallback={
                            <div class="room-list-empty">
                                <p>
                                    {props.client.rooms().length > 0
                                        ? "No rooms match this search."
                                        : "No rooms yet."}
                                </p>
                                <Show when={!query()}>
                                    <button
                                        class="ghost-button"
                                        type="button"
                                        onClick={props.onAddRoom}
                                    >
                                        Create or join one
                                    </button>
                                </Show>
                            </div>
                        }
                    >
                        <For each={filteredRooms()}>
                            {(room) => (
                                <button
                                    type="button"
                                    class="room-item"
                                    classList={{
                                        active: props.client.activePub() === room.pub,
                                    }}
                                    onClick={() => {
                                        void props.client.selectRoom(room.pub);
                                        props.onClose();
                                    }}
                                >
                                    <span
                                        class="room-avatar"
                                        style={{
                                            "--hue": `${avatarHue(room.pub)}`,
                                        }}
                                    >
                                        {room.name.slice(0, 1).toUpperCase()}
                                    </span>
                                    <span class="room-copy">
                                        <span class="room-name">{room.name}</span>
                                        <span class="room-preview">
                                            {props.client.roomSummaries()[room.pub]
                                                ?.preview || shortKey(room.pub, 6)}
                                        </span>
                                    </span>
                                    <span class="room-meta">
                                        <span class="room-time">
                                            {props.client.roomSummaries()[room.pub]?.ts
                                                ? formatTime(
                                                      props.client.roomSummaries()[
                                                          room.pub
                                                      ].ts,
                                                  )
                                                : ""}
                                        </span>
                                        <Show when={room.roomIdentity}>
                                            <ShieldIcon
                                                size={13}
                                                class="room-key-icon"
                                                aria-label="You hold this room key"
                                            />
                                        </Show>
                                    </span>
                                </button>
                            )}
                        </For>
                    </Show>
                </nav>

                <footer class="sidebar-foot">
                    <div class="identity-chip">
                        <span
                            class="identity-avatar"
                            style={{ "--hue": `${avatarHue(identity()?.sigPub ?? "")}` }}
                        >
                            {(identity()?.sigPub ?? "?").slice(0, 1).toUpperCase()}
                        </span>
                        <span class="identity-copy">
                            <span class="identity-label">This device</span>
                            <span class="identity-key">
                                {shortKey(identity()?.sigPub ?? "", 8)}
                            </span>
                        </span>
                    </div>
                    <button
                        type="button"
                        class="icon-button"
                        aria-label="Settings"
                        onClick={props.onSettings}
                    >
                        <SettingsIcon />
                    </button>
                </footer>
            </aside>
        </>
    );
}
