# PhoneConnect

PhoneConnect lets you trigger a cellular call from your laptop by relaying
commands through a small WebSocket gateway and a companion Android app
running on a nearby phone. The typical deployment looks like this:

```
Laptop CLI ⇄ WebSocket Gateway (node.js) ⇄ Wi‑Fi ⇄ Android App → Phone Call
```

- **CLI (`client/` in Rust)**: simple `dial` command‑line tool used on a laptop
  or any machine that can reach the gateway. It discovers the gateway via
  mDNS and sends HTTP requests.
- **Gateway (`server/` in Node.js)**: single‑process Express server exposing a
  REST API and a WebSocket endpoint. Android devices connect over WebSocket
  and register themselves. CLI callers POST to `/call` which are forwarded as
  JSON commands to the appropriate device.
- **Android App (`PhoneConnect/` in Kotlin/Gradle)**: foreground service keeps a
  persistent WebSocket to the gateway. Incoming `CALL` messages launch the
  dialer; call state is reported back over the socket. The app also performs
  mDNS discovery so the CLI/gateway can be located automatically.

## Architecture & Flow

1. **Discovery** – the gateway advertises
   `_phoneconnect._tcp.local.` via mDNS; both the CLI and the Android app
   browse for the same service type. When a gateway is found the CLI saves the
   HTTP/WS URL in `~/.config/phoneconnect/config.toml` so subsequent invocations
   are instant.
2. **Authentication** – every request uses a bearer token. The server reads
   allowed tokens from the `GATEWAY_TOKENS` environment variable (comma‑separated).
   The Android app supplies its token as part of the `AUTH` WebSocket message;
   the CLI uses it in the `Authorization` HTTP header.
3. **Call flow** – the CLI issues `POST /call { deviceId, number }`. If the
   target device is connected the gateway generates a `CALL` payload with a
   UUID, marks it logged, and sends it over the device’s WebSocket. The device
   acknowledges with `ACK` and then dials the given number. Call status
   messages (`CALL_STARTED`, `CALL_ENDED`, `CALL_FAILED`) are reported back
   and may be surfaced to future REST endpoints (currently logged).

## Components

### 1. CLI (Rust)

Path: `client/` — produces binary `dial`.

- Uses `clap` for command parsing (`call`, `devices`, `status`, `discover`,
  `config`).
- mDNS discovery via `mdns-sd` crate, asynchronous via Tokio.
- Configuration stored at
  `~/.config/phoneconnect/config.toml` with `server_url` and `token`.
- Implements simple E.164 phone validation and helpful coloured output.

### 2. Gateway Server (Node.js)

Path: `server/`.

- Express + `ws` for WebSocket handling.
- REST routes in `src/routes/call.js` provide `/call`, `/devices`, `/health`.
- Connection management (heartbeat, single‑connection per device) in
  `connectionManager.js`.
- Messages parsed/handled in `messageHandler.js`; contains helpers to build
  outbound commands.
- mDNS advertisement with `bonjour-service` so Android apps auto‑discover the
  gateway.
- Rate limiting, validation (`express-validator`), structured logging with
  `pino`.
- Authentication middleware in `auth.js`.
- Logs & config courtesy of `.env` and `pino-pretty` in dev.

### 3. Android App (Kotlin)

Path: `PhoneConnect/` (Gradle project).

- Jetpack Compose UI with three tabs: Home, Logs, Settings.
- Foreground `WsService` maintains WebSocket using `OkHttp` with automatic
  reconnection, heartbeat, and JSON message handling.
- `GatewayDiscovery` uses Android `NsdManager` to discover the gateway.
- `CallManager` triggers telephone calls via `ACTION_CALL` and listens for
  telephony state changes to report lifecycle events back to the gateway.
- Preferences stored with Jetpack DataStore; includes server URL,
  `deviceId` (UUID prefixed with `android_…`), and auth token.
- Service bus (`ServiceBus`) propagates state to the Compose UI without
  binding.
- Optional `BootReceiver` restarts the service on device boot.

## Getting started

### Prerequisites

- Rust toolchain (`cargo`) for CLI.
- Node 18+ and pnpm/npm for the server.
- Android Studio / SDK for the mobile app (min SDK 31).

### Server

```bash
git clone …/PhoneConnect server
cd server
pnpm install    # or npm install
dotenv config (create .env)
# example .env:
# PORT=3000
# GATEWAY_TOKENS=secret-android,secret-cli

npm run dev     # or pnpm dev
# production build: npm run build && node dist/server.js
```

### CLI

```bash
cd client
cargo build --release
# copy target/release/dial to your PATH

# initial config
dial config init
# edit ~/.config/phoneconnect/config.toml and set token & (optionally)
# server_url or run `dial discover` after the gateway is running

# make a call:
dial call android_abc123 +1234567890

# other commands:
dial devices
dial status
dial discover
```

### Android App

Open the `PhoneConnect` directory in Android Studio. Build & run on a device
with `CALL_PHONE` and `READ_PHONE_STATE` permissions. On first launch the app
will request runtime permissions and start the foreground service.

- The home screen shows connection status, device ID, and server URL.
- Settings allow manual server URL/token entry or scanning via mDNS.
- Logs tab displays WebSocket events for debugging.

You can also configure the app to start automatically on boot (requires
`RECEIVE_BOOT_COMPLETED` permission).

## Configuration

| Component | Config file / env | Default placeholder |
|-----------|-------------------|---------------------|
| CLI       | `~/.config/phoneconnect/config.toml` | `PLACEHOLDER_URL` (`http://10.61.214.187:3000`) |
| Server    | `.env` / environment | `PORT=3000`, `GATEWAY_TOKENS` | 
| Android   | DataStore prefs         | `ws://192.168.1.100:3000/ws` |

The CLI and Android use mDNS to avoid manual IP entry; if discovery fails you
can set the URL directly.

## Security

- All traffic is protected by a bearer token; rotate by changing
  `GATEWAY_TOKENS` and updating clients.
- WebSocket connections are authenticated before a device may register.
- Tokens are redacted from logs by the server logger configuration.

## Development notes

- Server logs are emitted as pretty‑printed text in development, JSON in
  production.
- The Rust client’s discovery code runs in a blocking thread and respects a
  configurable timeout.
- The Android app avoids duplicate commands using a bounded LRU cache.

## Future ideas

- Expose a server‑side status SSE endpoint for rich CLI output or web UI.
- Persist call history on the Android device.
- Add TLS support to the gateway and clients.

---

© 2025–2026 **PhoneConnect** — A lightweight bridge between your laptop and
your phone.{