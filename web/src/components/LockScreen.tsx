import { Show, createSignal, type JSX } from "solid-js";
import type { TolooClient } from "../lib/client";
import { LogoIcon } from "./Icons";

export function LockScreen(props: {
    client: TolooClient;
    onUnlocked: () => void;
}): JSX.Element {
    const [passphrase, setPassphrase] = createSignal("");
    const [confirmation, setConfirmation] = createSignal("");
    const [busy, setBusy] = createSignal(false);
    const [error, setError] = createSignal("");

    const setup = () => props.client.lockMode() === "setup";

    async function submit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (busy() || !passphrase()) return;
        if (setup() && passphrase() !== confirmation()) {
            setError("The passphrases do not match.");
            return;
        }
        setError("");
        setBusy(true);
        try {
            if (setup()) {
                await props.client.setupVault(passphrase());
            } else {
                await props.client.unlockVault(passphrase());
            }
            setPassphrase("");
            setConfirmation("");
            props.onUnlocked();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <div class="lock-screen">
            <form class="lock-card" onSubmit={(event) => void submit(event)}>
                <LogoIcon size={48} class="boot-logo" />
                <div class="lock-copy">
                    <h1>{setup() ? "Protect this device" : "Unlock Toloo"}</h1>
                    <p class="help">
                        {setup()
                            ? "Choose a passphrase to encrypt your identity and room keys on this device. It cannot be recovered."
                            : "Enter your passphrase to decrypt your local identity and room keys."}
                    </p>
                </div>
                <input
                    type="password"
                    autocomplete={setup() ? "new-password" : "current-password"}
                    placeholder="Passphrase"
                    autofocus
                    value={passphrase()}
                    onInput={(event) => {
                        setPassphrase(event.currentTarget.value);
                        setError("");
                    }}
                />
                <Show when={setup()}>
                    <input
                        type="password"
                        autocomplete="new-password"
                        placeholder="Confirm passphrase"
                        value={confirmation()}
                        onInput={(event) => {
                            setConfirmation(event.currentTarget.value);
                            setError("");
                        }}
                    />
                </Show>
                <Show when={error()}>
                    <p class="form-error" role="alert">
                        {error()}
                    </p>
                </Show>
                <button
                    type="submit"
                    class="primary-button wide"
                    disabled={
                        busy() ||
                        !passphrase() ||
                        (setup() && passphrase() !== confirmation())
                    }
                >
                    {busy()
                        ? "Working…"
                        : setup()
                          ? "Encrypt and continue"
                          : "Unlock"}
                </button>
            </form>
        </div>
    );
}
