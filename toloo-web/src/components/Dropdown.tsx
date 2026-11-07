import { For, Show, onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";

export interface DropdownEntry {
    label?: string;
    icon?: JSX.Element;
    danger?: boolean;
    warn?: boolean;
    success?: boolean;
    disabled?: boolean;
    separator?: boolean;
    onSelect?: () => void;
}

export function DropdownMenu(props: {
    x: number;
    y: number;
    entries: DropdownEntry[];
    onClose: () => void;
}): JSX.Element {
    let menu: HTMLDivElement | undefined;

    onMount(() => {
        const onKey = (event: KeyboardEvent) => {
            if (event.key === "Escape") props.onClose();
        };
        const onPointer = (event: PointerEvent) => {
            if (menu && !menu.contains(event.target as Node)) props.onClose();
        };
        document.addEventListener("keydown", onKey);
        document.addEventListener("pointerdown", onPointer, true);
        queueMicrotask(() => {
            if (!menu) return;
            const rect = menu.getBoundingClientRect();
            menu.style.left = `${Math.max(8, Math.min(props.x, window.innerWidth - rect.width - 8))}px`;
            menu.style.top = `${Math.max(8, Math.min(props.y, window.innerHeight - rect.height - 8))}px`;
        });
        onCleanup(() => {
            document.removeEventListener("keydown", onKey);
            document.removeEventListener("pointerdown", onPointer, true);
        });
    });

    return (
        <Portal>
            <div
                ref={menu}
                class="dropdown"
                role="menu"
                style={{ left: `${props.x}px`, top: `${props.y}px` }}
            >
                <For each={props.entries}>
                    {(entry) => (
                        <Show
                            when={!entry.separator}
                            fallback={<div class="dropdown-separator" />}
                        >
                            <button
                                type="button"
                                class="dropdown-item"
                                classList={{
                                    danger: entry.danger,
                                    warn: entry.warn,
                                    success: entry.success,
                                }}
                                role="menuitem"
                                disabled={entry.disabled}
                                onClick={() => {
                                    props.onClose();
                                    entry.onSelect?.();
                                }}
                            >
                                {entry.icon}
                                <span>{entry.label}</span>
                            </button>
                        </Show>
                    )}
                </For>
            </div>
        </Portal>
    );
}

export function menuPosition(element: HTMLElement): {
    x: number;
    y: number;
} {
    const rect = element.getBoundingClientRect();
    const rtl = document.documentElement.dir === "rtl";
    return { x: rtl ? rect.left : rect.right - 224, y: rect.bottom + 2 };
}
