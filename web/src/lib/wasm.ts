import init, {
    RoomRules as WasmRoomRules,
    buildMembership as wasmBuildMembership,
    avatarSpec as wasmAvatarSpec,
    canonicalize as wasmCanonicalize,
    coreVersion as wasmCoreVersion,
    decodeDiscovery as wasmDecodeDiscovery,
    encodeFile as wasmEncodeFile,
    encodeUri as wasmEncodeUri,
    envelopeInfo as wasmEnvelopeInfo,
    generateIdentity as wasmGenerateIdentity,
    generateRoomIdentity as wasmGenerateRoomIdentity,
    importIdentity as wasmImportIdentity,
    makeRoomCreate as wasmMakeRoomCreate,
    makeRoomDelete as wasmMakeRoomDelete,
    makeRoomFlag as wasmMakeRoomFlag,
    makeRoomJoin as wasmMakeRoomJoin,
    makeRoomLeave as wasmMakeRoomLeave,
    makeRoomMessage as wasmMakeRoomMessage,
    makeRoomUpdate as wasmMakeRoomUpdate,
    makeSideAttestation as wasmMakeSideAttestation,
    requestPoolExchange as wasmRequestPoolExchange,
    verifyChain as wasmVerifyChain,
    verifyChains as wasmVerifyChains,
    wrapCommit as wasmWrapCommit,
} from "../generated/toloo-wasm/toloo_wasm.js";
import wasmUrl from "../generated/toloo-wasm/toloo_wasm_bg.wasm?url";

export interface Identity {
    sigPub: string;
    sigSeed: string;
    encPub: string;
    encPriv: string;
}

export interface RoomIdentity {
    sigPub: string;
    sigSeed: string;
}

export interface Envelope {
    d: Record<string, unknown>;
    p: string;
}

export interface EnvelopeInfo {
    eid: string;
    datumId: string;
    depth: number;
    type: string;
    channel: number | null;
    ts: number;
    signer: string;
    room: string | null;
    to: string | null;
    tc: number | null;
    content: Record<string, unknown> | null;
}

export interface Verdict {
    ok: boolean;
    error?: string;
}

export interface AvatarSpec {
    cells: number;
    shape: number;
    palette: number;
    grid: boolean[];
    svgPath: string;
}

export interface RoomCreatePair {
    created: Envelope;
    committed: Envelope;
}

export interface PoolExchangePayload {
    offer: Envelope[];
    want: {
        rooms: string[];
        types?: string[];
        after?: number;
        limit?: number;
    };
}

let ready: Promise<void> | null = null;

export function initWasm(): Promise<void> {
    if (ready) return ready;
    const pending = init({ module_or_path: wasmUrl }).then(() => undefined);
    ready = pending;
    return pending;
}

export function coreVersion(): string {
    return wasmCoreVersion();
}

export function avatarSpec(publicKey: string): AvatarSpec {
    return JSON.parse(wasmAvatarSpec(publicKey)) as AvatarSpec;
}

export function generateIdentity(): Identity {
    return JSON.parse(wasmGenerateIdentity()) as Identity;
}

export function importIdentity(sigSeed: string, encPriv: string): Identity {
    return JSON.parse(wasmImportIdentity(sigSeed, encPriv)) as Identity;
}

export function generateRoomIdentity(): RoomIdentity {
    return JSON.parse(wasmGenerateRoomIdentity()) as RoomIdentity;
}

export function roomCreate(
    identity: Identity,
    room: RoomIdentity,
    name: string | null,
    rules: unknown[],
): RoomCreatePair {
    return JSON.parse(
        wasmMakeRoomCreate(
            JSON.stringify(identity),
            JSON.stringify(room),
            name,
            JSON.stringify(rules),
        ),
    ) as RoomCreatePair;
}

export function roomJoin(identity: Identity, roomPub: string): Envelope {
    return JSON.parse(
        wasmMakeRoomJoin(JSON.stringify(identity), roomPub, null),
    ) as Envelope;
}

export function roomMessage(
    identity: Identity,
    roomPub: string,
    channel: number,
    body: string,
): Envelope {
    return JSON.parse(
        wasmMakeRoomMessage(
            JSON.stringify(identity),
            roomPub,
            channel,
            body,
            null,
        ),
    ) as Envelope;
}

export function commitEnvelope(
    envelope: Envelope,
    room: RoomIdentity,
    tc: number,
): Envelope {
    return JSON.parse(
        wasmWrapCommit(JSON.stringify(envelope), JSON.stringify(room), tc),
    ) as Envelope;
}

export function info(envelope: Envelope | string): EnvelopeInfo {
    return JSON.parse(
        wasmEnvelopeInfo(
            typeof envelope === "string" ? envelope : JSON.stringify(envelope),
        ),
    ) as EnvelopeInfo;
}

export function verifyEnvelope(envelope: Envelope | string): Verdict {
    return JSON.parse(
        wasmVerifyChain(
            typeof envelope === "string" ? envelope : JSON.stringify(envelope),
        ),
    ) as Verdict;
}

export function verifyEnvelopes(envelopes: Envelope[]): Verdict[] {
    return JSON.parse(wasmVerifyChains(JSON.stringify(envelopes))) as Verdict[];
}

export function poolExchangeRequest(
    identity: Identity,
    payload: PoolExchangePayload,
): string {
    return wasmRequestPoolExchange(
        JSON.stringify(identity),
        JSON.stringify(payload),
    );
}

export function encodeInvite(envelopes: Envelope[]): string {
    return wasmEncodeUri(JSON.stringify(envelopes));
}

export function encodeFile(envelopes: Envelope[]): string {
    return wasmEncodeFile(JSON.stringify(envelopes));
}

export function decodeInvite(input: string): Envelope[] {
    return JSON.parse(wasmDecodeDiscovery(input)) as Envelope[];
}

export function roomLeave(identity: Identity, roomPub: string): Envelope {
    return JSON.parse(
        wasmMakeRoomLeave(JSON.stringify(identity), roomPub),
    ) as Envelope;
}

export function roomUpdate(
    identity: Identity,
    room: RoomIdentity,
    name: string,
): Envelope {
    return JSON.parse(
        wasmMakeRoomUpdate(
            JSON.stringify(identity),
            JSON.stringify({ sigPub: room.sigPub, sigSeed: room.sigSeed }),
            name,
            null,
        ),
    ) as Envelope;
}

export function roomDelete(
    identity: Identity,
    roomPub: string,
    channel: number,
    targetEid: string,
): Envelope {
    return JSON.parse(
        wasmMakeRoomDelete(JSON.stringify(identity), roomPub, channel, targetEid),
    ) as Envelope;
}

export function roomFlag(
    identity: Identity,
    roomPub: string,
    targetEid: string,
    category: string,
): Envelope {
    return JSON.parse(
        wasmMakeRoomFlag(
            JSON.stringify(identity),
            roomPub,
            targetEid,
            category,
            null,
        ),
    ) as Envelope;
}

export function sideAttestation(
    identity: Identity,
    target: string,
    level: "positive" | "negative" | "neutral",
): Envelope {
    return JSON.parse(
        wasmMakeSideAttestation(JSON.stringify(identity), target, level, null),
    ) as Envelope;
}

export function membership(
    roomPub: string,
    envelopes: Envelope[],
): { members: string[]; banned: string[] } {
    return JSON.parse(
        wasmBuildMembership(roomPub, JSON.stringify(envelopes)),
    ) as { members: string[]; banned: string[] };
}

export function canonicalize(value: unknown): string {
    return wasmCanonicalize(JSON.stringify(value));
}

export { WasmRoomRules as RoomRules };
