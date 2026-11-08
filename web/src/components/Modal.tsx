import { type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { CloseIcon } from "./Icons";

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

export function FeedbackModal(props: {
    message: string;
    onClose: () => void;
}): JSX.Element {
    return (
        <Modal title="Notice" onClose={props.onClose}>
            <p class="help">{props.message}</p>
            <button type="button" class="ghost-button wide" onClick={props.onClose}>
                Close
            </button>
        </Modal>
    );
}
