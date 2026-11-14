import { createSignal, type JSX } from "solid-js";
import type { TolooClient } from "../lib/client";
import { Modal } from "./Modal";

export function NewDmModal(props: {
    client: TolooClient;
    onClose: () => void;
    onCreated: (peer: string) => void;
}): JSX.Element {
    const [sigPub, setSigPub] = createSignal("");
    const [encPub, setEncPub] = createSignal("");
    const [name, setName] = createSignal("");
    const [busy, setBusy] = createSignal(false);
    const [error, setError] = createSignal("");

    async function submit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (busy()) return;
        setError("");
        setBusy(true);
        try {
            const contact = await props.client.addDmContact(
                sigPub(),
                encPub(),
                name(),
            );
            props.onCreated(contact.sigPub);
            props.onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setBusy(false);
        }
    }

    return (
        <Modal title="New direct message" onClose={props.onClose}>
            <form class="stack" onSubmit={(event) => void submit(event)}>
                <p class="help">
                    Ask your contact for both public keys. Messages are
                    encrypted locally and delivered through your relays.
                </p>
                <input
                    type="text"
                    spellcheck={false}
                    autocomplete="off"
                    placeholder="Signing public key"
                    value={sigPub()}
                    onInput={(event) => setSigPub(event.currentTarget.value)}
                />
                <input
                    type="text"
                    spellcheck={false}
                    autocomplete="off"
                    placeholder="Encryption public key"
                    value={encPub()}
                    onInput={(event) => setEncPub(event.currentTarget.value)}
                />
                <input
                    type="text"
                    spellcheck={false}
                    placeholder="Display name (optional)"
                    value={name()}
                    onInput={(event) => setName(event.currentTarget.value)}
                />
                {error() && <p class="form-error">{error()}</p>}
                <div class="button-row end">
                    <button
                        class="ghost-button"
                        type="button"
                        onClick={props.onClose}
                    >
                        Cancel
                    </button>
                    <button
                        class="primary-button"
                        type="submit"
                        disabled={busy() || !sigPub().trim() || !encPub().trim()}
                    >
                        {busy() ? "Adding…" : "Start conversation"}
                    </button>
                </div>
            </form>
        </Modal>
    );
}
