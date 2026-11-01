# toloo-web

SolidJS browser client for Toloo. Protocol work runs in the shared
[`toloo-wasm`](../toloo-wasm/) core; WebSocket/HTTP transport, IndexedDB
storage, and UI live in TypeScript.

## Development

Requirements: Rust + `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.126,
Node 22+.

```sh
# from the repository root
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.126 --locked

cd toloo-web
npm install
npm run dev      # builds/copies the wasm package, then starts Vite on :5173
npm test         # transport policy + HTTP transport tests
npm run build    # wasm + tsc + production bundle in dist/
```

For a local relay:

```sh
# from the repository root
cargo run -p toloo-cli -- keygen > /tmp/toloo-relay-id.json
cargo run -p toloo-cli -- relay \
  --host 127.0.0.1 --port 17701 \
  --http-port 17702 --http-path /toloo \
  --identity /tmp/toloo-relay-id.json \
  --db /tmp/toloo-web-pool
```

Then add `ws://127.0.0.1:17701` or `http://127.0.0.1:17702/toloo` in
Settings. An `http://localhost:5173` page accepts only `http`/`ws`; an `https`
deployment accepts only `https`/`wss`. The HTTP endpoint emits CORS preflight
and response headers for browser use.

## Layers

- **`src/lib/wasm.ts`** — typed bridge over the generated JSON contract.
- **`src/lib/transport.ts`** — WebSocket/HTTP relay adapters and the strict
  page-scheme policy. WebSocket connections persist and serialize requests;
  HTTP uses one POST per request.
- **`src/lib/db.ts`** — IndexedDB stores for identity, relays, rooms, and
  envelopes.
- **`src/lib/client.ts`** — sync, batch verification, envelope storage,
  membership, room authority auto-commit, and message actions.
- **`src/components/`** — onboarding, sidebar, chat, settings, and room modals.

## Current scope and limitations

- Identity and room private keys are stored unencrypted in IndexedDB. There is
  no passphrase-wrapped keystore yet.
- A browser with a room key commits pending joins/messages while its tab is
  open (polling every 12 seconds). If the key-holder tab is offline, events
  stay uncommitted; the relay now exposes those events through `pool.exchange`
  and `events.sync` per spec §7.7.3.
- Group chat, room creation/join, invites, and member counts work. Direct
  messages, reactions/edits UI, files/blobs, notifications, offline/service
  worker support, and moderation UI are not implemented yet.
- The hosted client is CA-TLS dependent; self-signed certificates are a native
  client concern. HTTP hosting is dev/LAN only.
- The room creator is not counted in `members` by the core membership
  algorithm; only committed channel -2 joins are replayed.
