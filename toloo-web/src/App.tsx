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
import {
    AddRoomModal,
    ImportModal,
    InviteModal,
    RenameModal,
    SettingsModal,
} from "./components/Modals";
import { Sidebar } from "./components/Sidebar";
import { FeedbackModal } from "./components/Modal";
import { LogoIcon } from "./components/Icons";

export default function App(): JSX.Element {
    const client = createTolooClient();
    const [bootError, setBootError] = createSignal("");
    const [mobilePage, setMobilePage] = createSignal<"rooms" | "chat">(
        "rooms",
    );
    const [settingsOpen, setSettingsOpen] = createSignal(false);
    const [addRoomOpen, setAddRoomOpen] = createSignal(false);
    const [importOpen, setImportOpen] = createSignal(false);
    const [renameRoom, setRenameRoom] = createSignal<RoomRecord | null>(null);
    const [inviteRoom, setInviteRoom] = createSignal<RoomRecord | null>(null);

    onMount(() => {
        document.documentElement.dir =
            localStorage.getItem("toloo-rtl") === "1" ? "rtl" : "ltr";
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

    function openAddRoom(): void {
        setAddRoomOpen(true);
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
                                <LogoIcon size={48} class="boot-logo" />
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
            <div class="app-shell" data-page={mobilePage()}>
                <Sidebar
                    client={client}
                    onAddRoom={openAddRoom}
                    onImportRoom={() => setImportOpen(true)}
                    onSettings={() => setSettingsOpen(true)}
                    onRenameRoom={(room) => setRenameRoom(room)}
                    onShareRoom={(room) => setInviteRoom(room)}
                    onOpenChat={() => setMobilePage("chat")}
                />
                <Chat
                    client={client}
                    onBack={() => setMobilePage("rooms")}
                    onInvite={() => {
                        const room = client.activeRoom();
                        if (room) setInviteRoom(room);
                    }}
                    onRenameRoom={(room) => setRenameRoom(room)}
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
                    onCreated={() => setAddRoomOpen(false)}
                />
            </Show>
            <Show when={importOpen()}>
                <ImportModal
                    client={client}
                    onClose={() => setImportOpen(false)}
                />
            </Show>
            <Show when={renameRoom()}>
                {(room) => (
                    <RenameModal
                        client={client}
                        room={room()}
                        onClose={() => setRenameRoom(null)}
                    />
                )}
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
            <Show when={client.feedback()}>
                {(message) => (
                    <FeedbackModal
                        message={message()}
                        onClose={client.dismissFeedback}
                    />
                )}
            </Show>
        </Show>
    );
}
