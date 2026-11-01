import { For, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { CloseIcon } from "./Icons";
import type { Toast } from "../lib/client";

export function Modal(props: {
    title: string;
    subtitle?: string;
    onClose: () => void;
    children: JSX.Element;
    wide?: boolean;
}): JSX.Element {
    function onKeyDown(event: KeyboardEvent): void {
        if (event.key === "Escape") {
            event.preventDefault();
            props.onClose();
        }
    }

    function onBackdrop(event: MouseEvent): void {
        if (event.target === event.currentTarget) props.onClose();
    }

    return (
        <Portal>
            <div class="modal-backdrop" onMouseDown={onBackdrop}>
                <div
                    class="modal-panel"
                    classList={{ "modal-wide": props.wide }}
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="modal-title"
                    tabIndex={-1}
                    onKeyDown={onKeyDown}
                >
                    <header class="modal-head">
                        <div>
                            <h2 id="modal-title">{props.title}</h2>
                            {props.subtitle && <p>{props.subtitle}</p>}
                        </div>
                        <button
                            class="icon-button"
                            type="button"
                            aria-label="Close"
                            onClick={props.onClose}
                        >
                            <CloseIcon />
                        </button>
                    </header>
                    <div class="modal-body">{props.children}</div>
                </div>
            </div>
        </Portal>
    );
}

export function Toasts(props: {
    toasts: Toast[];
    dismiss: (id: string) => void;
}): JSX.Element {
    return (
        <Portal>
            <div class="toasts" aria-live="polite" aria-atomic="false">
                <For each={props.toasts}>
                    {(toast) => (
                        <button
                            type="button"
                            class="toast"
                            classList={{
                                "toast-error": toast.kind === "error",
                                "toast-success": toast.kind === "success",
                            }}
                            onClick={() => props.dismiss(toast.id)}
                        >
                            <span class="toast-dot" aria-hidden="true" />
                            <span>{toast.text}</span>
                        </button>
                    )}
                </For>
            </div>
        </Portal>
    );
}
