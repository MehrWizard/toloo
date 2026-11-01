import {
    Show,
    createSignal,
    onCleanup,
    onMount,
    type JSX,
} from "solid-js";
import { createTolooClient } from "./lib/client";
import type { RoomRecord } from "./lib/db";
import { Chat } from "./components/Chat";
import { AddRoomModal, InviteModal, SettingsModal } from "./components/Modals";
import { Onboarding } from "./components/Onboarding";
import { Sidebar } from "./components/Sidebar";
import { Toasts } from "./components/Modal";

export default function App(): JSX.Element {
    const client = createTolooClient();
    const [bootError, setBootError] = createSignal("");
    const [sidebarOpen, setSidebarOpen] = createSignal(false);
    const [settingsOpen, setSettingsOpen] = createSignal(false);
    const [addRoomOpen, setAddRoomOpen] = createSignal(false);
    const [inviteRoom, setInviteRoom] = createSignal<RoomRecord | null>(null);

    onMount(() => {
        client
            .load()
            .then(() => {
                if (client.activePub()) void client.syncRoom(undefined, true);
            })
            .catch((error) => {
                setBootError(
                    error instanceof Error ? error.message : String(error),
                );
            });
        const stop = client.startAutoSync();
        const onVisible = () => {
            if (document.visibilityState === "visible" && client.activePub()) {
                void client.syncRoom(undefined, true);
            }
        };
        document.addEventListener("visibilitychange", onVisible);
        onCleanup(() => {
            stop();
            document.removeEventListener("visibilitychange", onVisible);
        });
    });

    function openInvite(room: RoomRecord): void {
        setAddRoomOpen(false);
        setInviteRoom(room);
    }

    return (
        <Show
            when={client.initialized()}
            fallback={
                <div class="boot-screen">
                    <Show
                        when={bootError()}
                        fallback={
                            <>
                                <div class="boot-mark" aria-hidden="true">
                                    T
                                </div>
                                <p>Opening the wasm core…</p>
                            </>
                        }
                    >
                        <h1>Could not open Toloo</h1>
                        <p class="form-error">{bootError()}</p>
                        <p class="help">
                            IndexedDB may be unavailable in this browser mode.
                            Try a normal window or disable private browsing.
                        </p>
                    </Show>
                </div>
            }
        >
            <Show
                when={client.identity()}
                fallback={<Onboarding client={client} />}
            >
                <div class="app-shell">
                    <Sidebar
                        client={client}
                        open={sidebarOpen()}
                        onClose={() => setSidebarOpen(false)}
                        onAddRoom={() => setAddRoomOpen(true)}
                        onSettings={() => setSettingsOpen(true)}
                    />
                    <Chat
                        client={client}
                        onMenu={() => setSidebarOpen(true)}
                        onInvite={() => {
                            const room = client.activeRoom();
                            if (room) setInviteRoom(room);
                        }}
                        onSettings={() => setSettingsOpen(true)}
                    />
                </div>

                <Show when={settingsOpen()}>
                    <SettingsModal
                        client={client}
                        onClose={() => setSettingsOpen(false)}
                    />
                </Show>
                <Show when={addRoomOpen()}>
                    <AddRoomModal
                        client={client}
                        onClose={() => setAddRoomOpen(false)}
                        onCreated={openInvite}
                    />
                </Show>
                <Show when={inviteRoom()}>
                    {(room) => (
                        <InviteModal
                            client={client}
                            room={room()}
                            onClose={() => setInviteRoom(null)}
                        />
                    )}
                </Show>
                <Toasts toasts={client.toasts()} dismiss={client.dismissToast} />
            </Show>
        </Show>
    );
}
