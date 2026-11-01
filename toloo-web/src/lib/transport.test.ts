import { afterEach, describe, expect, it, vi } from "vitest";
import {
    allowedProtocols,
    parseEndpoint,
    RelayConnection,
} from "./transport";

afterEach(() => {
    vi.unstubAllGlobals();
});

describe("browser transport policy", () => {
    it("pairs schemes with the page protocol", () => {
        expect(allowedProtocols("https:")).toEqual(["https", "wss"]);
        expect(allowedProtocols("http:")).toEqual(["http", "ws"]);
    });

    it("accepts secure endpoints on HTTPS pages", () => {
        expect(parseEndpoint("wss://relay.example/toloo", "https:").proto).toBe(
            "wss",
        );
        expect(parseEndpoint("https://relay.example/toloo", "https:").proto).toBe(
            "https",
        );
    });

    it("rejects insecure endpoints on HTTPS pages", () => {
        expect(() => parseEndpoint("ws://relay.example", "https:")).toThrow(
            /HTTPS.*insecure/,
        );
        expect(() => parseEndpoint("http://relay.example", "https:")).toThrow(
            /HTTPS.*insecure/,
        );
    });

    it("rejects secure endpoints on HTTP pages by policy", () => {
        expect(() => parseEndpoint("wss://relay.example", "http:")).toThrow(
            /HTTP.*only accepts/,
        );
        expect(() => parseEndpoint("https://relay.example", "http:")).toThrow(
            /HTTP.*only accepts/,
        );
    });

    it("defaults bare hosts to the page's websocket scheme", () => {
        expect(parseEndpoint("127.0.0.1:7700", "http:")).toMatchObject({
            proto: "ws",
            url: "ws://127.0.0.1:7700/",
        });
        expect(parseEndpoint("relay.example/toloo", "https:")).toMatchObject({
            proto: "wss",
            url: "wss://relay.example/toloo",
        });
    });

    it("rejects unknown schemes and credentials", () => {
        expect(() => parseEndpoint("ftp://relay.example", "http:")).toThrow(
            /Relay URL must use/,
        );
        expect(() =>
            parseEndpoint("ws://user:pass@relay.example", "http:"),
        ).toThrow(/credentials/);
    });
});

describe("HTTP relay transport", () => {
    it("posts the envelope and returns response envelopes", async () => {
        const envelope = { d: { n: "relay", t: "node.meta" }, p: "signature" };
        const fetchMock = vi.fn(
            async (_input: RequestInfo | URL, _init?: RequestInit) =>
                new Response(JSON.stringify([envelope]), {
                    status: 200,
                    headers: { "content-type": "application/json" },
                }),
        );
        vi.stubGlobal("fetch", fetchMock);
        const connection = new RelayConnection(
            { proto: "http", url: "http://127.0.0.1:7700/" },
            "http:",
        );
        const result = await connection.request('{"d":{},"p":""}');
        expect(result).toEqual([JSON.stringify(envelope)]);
        expect(fetchMock).toHaveBeenCalledOnce();
        expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({
            method: "POST",
            body: '{"d":{},"p":""}',
        });
    });

    it("surfaces relay error objects", async () => {
        vi.stubGlobal(
            "fetch",
            vi.fn(async () =>
                new Response(JSON.stringify([{ error: "invalid_request" }]), {
                    status: 200,
                }),
            ),
        );
        const connection = new RelayConnection(
            { proto: "http", url: "http://127.0.0.1:7700/" },
            "http:",
        );
        await expect(connection.request("{}")).rejects.toThrow("invalid_request");
    });

    it("rejects non-array responses", async () => {
        vi.stubGlobal(
            "fetch",
            vi.fn(async () => new Response("{}", { status: 200 })),
        );
        const connection = new RelayConnection(
            { proto: "http", url: "http://127.0.0.1:7700/" },
            "http:",
        );
        await expect(connection.request("{}")).rejects.toThrow(/unexpected/);
    });
});
