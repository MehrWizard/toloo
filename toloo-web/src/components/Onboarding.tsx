import { Show, createSignal, type JSX } from "solid-js";
import type { TolooClient } from "../lib/client";
import { LockIcon, ShieldIcon } from "./Icons";

export function Onboarding(props: { client: TolooClient }): JSX.Element {
    const [mode, setMode] = createSignal<"create" | "restore">("create");
    const [sigSeed, setSigSeed] = createSignal("");
    const [encPriv, setEncPriv] = createSignal("");
    const [busy, setBusy] = createSignal(false);
    const [error, setError] = createSignal("");

    async function create(): Promise<void> {
        setBusy(true);
        setError("");
        try {
            await props.client.createIdentity();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    async function restore(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        setBusy(true);
        setError("");
        try {
            await props.client.restoreIdentity(sigSeed(), encPriv());
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <main class="onboarding">
            <div class="onboarding-glow" aria-hidden="true" />
            <section class="onboarding-intro">
                <div class="wordmark">
                    <span class="wordmark-mark" aria-hidden="true">
                        T
                    </span>
                    <span>toloo</span>
                </div>
                <h1>
                    Conversation that
                    <br />
                    belongs to <em>you.</em>
                </h1>
                <p class="lede">
                    Self-verifying envelopes. Relays that store and forward.
                    No account, no central inbox, no live session to lose.
                </p>
                <ul class="feature-list">
                    <li>
                        <ShieldIcon size={19} />
                        <span>
                            <strong>Signed by you.</strong> Every message is an
                            Ed25519 envelope relays cannot forge.
                        </span>
                    </li>
                    <li>
                        <LockIcon size={19} />
                        <span>
                            <strong>Private when it matters.</strong> Direct
                            messages use X25519 + ChaCha20-Poly1305.
                        </span>
                    </li>
                    <li>
                        <span class="feature-glyph" aria-hidden="true">
                            ↯
                        </span>
                        <span>
                            <strong>Gapped by design.</strong> Recipients can be
                            offline for days; envelopes wait.
                        </span>
                    </li>
                </ul>
            </section>

            <section class="onboarding-card">
                <div class="segmented" role="tablist" aria-label="Identity">
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
                        New identity
                    </button>
                    <button
                        type="button"
                        role="tab"
                        aria-selected={mode() === "restore"}
                        classList={{ active: mode() === "restore" }}
                        onClick={() => {
                            setMode("restore");
                            setError("");
                        }}
                    >
                        Restore
                    </button>
                </div>

                <Show when={mode() === "create"}>
                    <div class="onboarding-copy">
                        <h2>Start with a keypair</h2>
                        <p>
                            Your signing and encryption keys are generated in
                            this browser by the Toloo wasm core. Nothing is sent
                            anywhere.
                        </p>
                    </div>
                    <button
                        class="primary-button wide"
                        type="button"
                        disabled={busy()}
                        onClick={() => void create()}
                    >
                        {busy() ? "Generating…" : "Create identity"}
                    </button>
                </Show>

                <Show when={mode() === "restore"}>
                    <form class="stack" onSubmit={(event) => void restore(event)}>
                        <label class="field">
                            <span>Signing seed</span>
                            <input
                                type="password"
                                autocomplete="off"
                                spellcheck={false}
                                required
                                placeholder="43-character base64url seed"
                                value={sigSeed()}
                                onInput={(event) => setSigSeed(event.currentTarget.value)}
                            />
                        </label>
                        <label class="field">
                            <span>Encryption private key</span>
                            <input
                                type="password"
                                autocomplete="off"
                                spellcheck={false}
                                required
                                placeholder="43-character base64url key"
                                value={encPriv()}
                                onInput={(event) => setEncPriv(event.currentTarget.value)}
                            />
                        </label>
                        <button class="primary-button wide" type="submit" disabled={busy()}>
                            {busy() ? "Restoring…" : "Restore identity"}
                        </button>
                    </form>
                </Show>

                <Show when={error()}>
                    <p class="form-error" role="alert">
                        {error()}
                    </p>
                </Show>

                <p class="fine-print">
                    Keys stay in this browser’s IndexedDB. Use a trusted device
                    and an HTTPS origin. Export a recovery copy from Settings.
                </p>
            </section>
        </main>
    );
}
