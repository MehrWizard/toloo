export type TransportProto = "ws" | "wss" | "http" | "https";

export interface RelayEndpoint {
    proto: TransportProto;
    url: string;
}

const PROTOCOLS: TransportProto[] = ["ws", "wss", "http", "https"];

export function isTransportProto(value: string): value is TransportProto {
    return (PROTOCOLS as string[]).includes(value);
}

export function pageProtocol(): string {
    return typeof window === "undefined" ? "http:" : window.location.protocol;
}

export function allowedProtocols(page = pageProtocol()): TransportProto[] {
    return page === "https:" ? ["https", "wss"] : ["http", "ws"];
}

export function defaultEndpointScheme(page = pageProtocol()): "ws" | "wss" {
    return page === "https:" ? "wss" : "ws";
}

export function parseEndpoint(raw: string, page = pageProtocol()): RelayEndpoint {
    let value = raw.trim();
    if (!value) throw new Error("Enter a relay address.");
    if (!value.includes("://")) {
        value = `${defaultEndpointScheme(page)}://${value}`;
    }
    let url: URL;
    try {
        url = new URL(value);
    } catch {
        throw new Error("That is not a valid relay URL.");
    }
    const proto = url.protocol.slice(0, -1);
    if (!isTransportProto(proto)) {
        throw new Error("Relay URL must use ws, wss, http, or https.");
    }
    if (url.username || url.password) {
        throw new Error("Relay credentials in the URL are not supported.");
    }
    const endpoint = { proto, url: url.toString() } satisfies RelayEndpoint;
    assertAllowed(endpoint, page);
    return endpoint;
}

export function assertAllowed(endpoint: RelayEndpoint, page = pageProtocol()): void {
    const allowed = allowedProtocols(page);
    if (allowed.includes(endpoint.proto)) return;
    if (page === "https:") {
        throw new Error(
            `This page is HTTPS, so insecure ${endpoint.proto}:// relays are blocked. Use wss:// or https://.`,
        );
    }
    throw new Error(
        `This page is HTTP, so it only accepts http:// and ws:// relays by policy, not ${endpoint.proto}://.`,
    );
}

function isEnvelope(value: unknown): value is { d: unknown; p: string } {
    return (
        typeof value === "object" &&
        value !== null &&
        !Array.isArray(value) &&
        "d" in value &&
        "p" in value
    );
}

function errorFromFrame(value: Record<string, unknown>): string {
    const code = typeof value.error === "string" ? value.error : "relay_error";
    const message = typeof value.message === "string" ? value.message : "";
    return message ? `${code}: ${message}` : code;
}

interface PendingRequest {
    resolve: (envelopes: string[]) => void;
    reject: (error: Error) => void;
    envelopes: string[];
    timer: number;
}

/**
 * One relay connection. WebSocket relays keep a persistent connection and
 * serialize requests; HTTP relays use one POST per request.
 */
export class RelayConnection {
    readonly endpoint: RelayEndpoint;
    private ws: WebSocket | null = null;
    private opening: Promise<WebSocket> | null = null;
    private pending: PendingRequest | null = null;
    private queue: Promise<void> = Promise.resolve();

    constructor(endpoint: RelayEndpoint, page = pageProtocol()) {
        assertAllowed(endpoint, page);
        this.endpoint = endpoint;
    }

    close(): void {
        this.failPending(new Error("Relay connection closed."));
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
        this.opening = null;
    }

    request(envelopeJson: string, timeoutMs = 20_000): Promise<string[]> {
        const run = this.queue.then(() => {
            if (this.endpoint.proto === "http" || this.endpoint.proto === "https") {
                return this.httpRequest(envelopeJson, timeoutMs);
            }
            return this.wsRequest(envelopeJson, timeoutMs);
        });
        this.queue = run.then(
            () => undefined,
            () => undefined,
        );
        return run;
    }

    private async httpRequest(envelopeJson: string, timeoutMs: number): Promise<string[]> {
        const controller = new AbortController();
        const timer = globalThis.setTimeout(() => controller.abort(), timeoutMs);
        try {
            const response = await fetch(this.endpoint.url, {
                method: "POST",
                headers: { "content-type": "application/json" },
                body: envelopeJson,
                signal: controller.signal,
                cache: "no-store",
            });
            if (!response.ok) {
                throw new Error(`Relay returned HTTP ${response.status}.`);
            }
            const data: unknown = await response.json();
            if (!Array.isArray(data)) {
                throw new Error("Relay returned an unexpected HTTP response.");
            }
            const out: string[] = [];
            for (const item of data) {
                if (isEnvelope(item)) {
                    out.push(JSON.stringify(item));
                } else if (typeof item === "object" && item !== null && "error" in item) {
                    throw new Error(errorFromFrame(item as Record<string, unknown>));
                }
            }
            return out;
        } catch (error) {
            if (error instanceof DOMException && error.name === "AbortError") {
                throw new Error("Relay request timed out.");
            }
            throw error;
        } finally {
            globalThis.clearTimeout(timer);
        }
    }

    private async wsRequest(envelopeJson: string, timeoutMs: number): Promise<string[]> {
        const ws = await this.connect();
        return new Promise<string[]>((resolve, reject) => {
            if (this.pending) {
                reject(new Error("A relay request is already in flight."));
                return;
            }
            const timer = globalThis.setTimeout(() => {
                this.failPending(new Error("Relay request timed out."));
            }, timeoutMs);
            this.pending = { resolve, reject, envelopes: [], timer };
            try {
                ws.send(envelopeJson);
            } catch (error) {
                this.failPending(
                    error instanceof Error ? error : new Error(String(error)),
                );
            }
        });
    }

    private connect(): Promise<WebSocket> {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            return Promise.resolve(this.ws);
        }
        if (this.opening) return this.opening;

        const socket = new WebSocket(this.endpoint.url);
        socket.binaryType = "arraybuffer";
        this.ws = socket;

        this.opening = new Promise<WebSocket>((resolve, reject) => {
            let settled = false;
            const fail = (error: Error) => {
                if (settled) return;
                settled = true;
                this.opening = null;
                if (this.ws === socket) this.ws = null;
                reject(error);
            };

            socket.addEventListener("open", () => {
                if (settled) return;
                settled = true;
                this.opening = null;
                resolve(socket);
            });
            socket.addEventListener("error", () => {
                fail(new Error(`Could not connect to ${this.endpoint.url}.`));
            });
            socket.addEventListener("close", (event) => {
                if (!settled) {
                    fail(
                        new Error(
                            `Relay closed before connecting (code ${event.code}).`,
                        ),
                    );
                }
                if (this.ws === socket) this.ws = null;
                this.failPending(new Error("Relay connection closed."));
            });
            socket.addEventListener("message", (event) => {
                this.handleFrame(event.data);
            });
        });

        return this.opening;
    }

    private handleFrame(data: string | ArrayBuffer | Blob): void {
        let text: string;
        if (typeof data === "string") {
            text = data;
        } else if (data instanceof ArrayBuffer) {
            text = new TextDecoder().decode(data);
        } else {
            return;
        }

        let value: unknown;
        try {
            value = JSON.parse(text);
        } catch {
            this.failPending(new Error("Relay sent malformed JSON."));
            return;
        }
        if (typeof value !== "object" || value === null || Array.isArray(value)) {
            return;
        }
        const frame = value as Record<string, unknown>;
        if (frame.done === true) {
            const pending = this.pending;
            if (!pending) return;
            this.pending = null;
            globalThis.clearTimeout(pending.timer);
            pending.resolve(pending.envelopes);
            return;
        }
        if (typeof frame.error === "string") {
            this.failPending(new Error(errorFromFrame(frame)));
            return;
        }
        if (typeof frame.wr === "number") return;
        if (isEnvelope(frame)) {
            this.pending?.envelopes.push(JSON.stringify(frame));
        }
    }

    private failPending(error: Error): void {
        const pending = this.pending;
        if (!pending) return;
        this.pending = null;
        globalThis.clearTimeout(pending.timer);
        pending.reject(error);
    }
}
