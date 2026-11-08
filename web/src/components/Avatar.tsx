import { Show, type JSX } from "solid-js";
import { AVATAR_PALETTE } from "../generated/design";
import * as protocol from "../lib/wasm";

const cache = new Map<string, protocol.AvatarSpec>();

function specFor(publicKey: string): protocol.AvatarSpec | null {
    if (!publicKey) return null;
    const cached = cache.get(publicKey);
    if (cached) return cached;
    try {
        const spec = protocol.avatarSpec(publicKey);
        cache.set(publicKey, spec);
        return spec;
    } catch {
        return null;
    }
}

export function Avatar(props: {
    publicKey: string;
    size?: number;
    class?: string;
    label?: string;
}): JSX.Element {
    const size = () => props.size ?? 36;
    const spec = () => specFor(props.publicKey);
    return (
        <span
            class={`pk-avatar${props.class ? ` ${props.class}` : ""}`}
            style={{
                width: `${size()}px`,
                height: `${size()}px`,
            }}
            title={props.label}
            aria-label={props.label}
        >
            <Show when={spec()}>
                {(avatar) => (
                    <svg viewBox="0 0 100 100" aria-hidden="true">
                        <path
                            d={avatar().svgPath}
                            fill={AVATAR_PALETTE[avatar().palette]}
                        />
                    </svg>
                )}
            </Show>
        </span>
    );
}
