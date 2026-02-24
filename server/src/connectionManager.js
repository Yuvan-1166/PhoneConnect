import { EventEmitter } from "events";
import logger from "./logger.js";

const log = logger.child({ module: "connectionManager" });

/** How long a socket may be silent before it is considered dead (ms). */
const PING_INTERVAL_MS = 30_000;
const PONG_TIMEOUT_MS = 10_000;

/**
 * Manages all live WebSocket connections from Android devices.
 *
 * Each device is identified by a stable `deviceId` string.
 * Multiple connections with the same deviceId are not allowed —
 * a new connection replaces the previous one gracefully.
 *
 * Emits:
 *   'connected'    (deviceId)
 *   'disconnected' (deviceId, reason)
 *   'message'      (deviceId, parsedObject)
 */
class ConnectionManager extends EventEmitter {
  constructor() {
    super();
    /** @type {Map<string, { ws: WebSocket, pingTimer: NodeJS.Timeout, pongTimer?: NodeJS.Timeout, connectedAt: Date }>} */
    this._devices = new Map();
  }

  // ── Registration ────────────────────────────────────────────────────────────

  /**
   * Register a new authenticated WebSocket for a device.
   * Replaces any existing connection for the same deviceId.
   */
  register(deviceId, ws) {
    // Evict existing connection if any
    if (this._devices.has(deviceId)) {
      log.info({ deviceId }, "Replacing existing connection");
      this._close(deviceId, "Replaced by new connection");
    }

    const entry = {
      ws,
      connectedAt: new Date(),
      pingTimer: null,
      pongTimer: null,
    };
    this._devices.set(deviceId, entry);

    ws.on("pong", () => this._onPong(deviceId));
    ws.on("close", (code, reason) => {
      this._clearTimers(deviceId);
      this._devices.delete(deviceId);
      log.info({ deviceId, code, reason: reason.toString() }, "Device disconnected");
      this.emit("disconnected", deviceId, reason.toString());
    });
    ws.on("error", (err) => {
      log.error({ deviceId, err: err.message }, "Socket error");
    });

    this._startHeartbeat(deviceId);
    log.info({ deviceId }, "Device registered");
    this.emit("connected", deviceId);
  }

  // ── Sending ─────────────────────────────────────────────────────────────────

  /**
   * Send a JSON payload to a specific device.
   * Returns true if sent, false if device is not connected.
   */
  sendTo(deviceId, payload) {
    const entry = this._devices.get(deviceId);
    if (!entry) {
      log.warn({ deviceId }, "sendTo: device not connected");
      return false;
    }
    const json = JSON.stringify(payload);
    try {
      entry.ws.send(json);
      log.debug({ deviceId, payload }, "→ device");
      return true;
    } catch (err) {
      log.error({ deviceId, err: err.message }, "Send failed");
      return false;
    }
  }

  // ── Queries ──────────────────────────────────────────────────────────────────

  isConnected(deviceId) {
    return this._devices.has(deviceId);
  }

  connectedDevices() {
    return [...this._devices.keys()];
  }

  stats() {
    const devices = [];
    for (const [id, entry] of this._devices) {
      devices.push({ deviceId: id, connectedAt: entry.connectedAt });
    }
    return { count: devices.length, devices };
  }

  // ── Heartbeat ────────────────────────────────────────────────────────────────

  _startHeartbeat(deviceId) {
    const entry = this._devices.get(deviceId);
    if (!entry) return;

    entry.pingTimer = setInterval(() => {
      const e = this._devices.get(deviceId);
      if (!e) return;

      // Expect a pong back within PONG_TIMEOUT_MS
      e.pongTimer = setTimeout(() => {
        log.warn({ deviceId }, "Pong timeout — terminating connection");
        e.ws.terminate();
        this._devices.delete(deviceId);
        this.emit("disconnected", deviceId, "pong timeout");
      }, PONG_TIMEOUT_MS);

      try {
        e.ws.ping();
      } catch (_) {
        // socket already dead
      }
    }, PING_INTERVAL_MS);
  }

  _onPong(deviceId) {
    const entry = this._devices.get(deviceId);
    if (!entry) return;
    clearTimeout(entry.pongTimer);
    entry.pongTimer = null;
    log.debug({ deviceId }, "Pong received");
  }

  _clearTimers(deviceId) {
    const entry = this._devices.get(deviceId);
    if (!entry) return;
    clearInterval(entry.pingTimer);
    clearTimeout(entry.pongTimer);
  }

  _close(deviceId, reason = "closed") {
    const entry = this._devices.get(deviceId);
    if (!entry) return;
    this._clearTimers(deviceId);
    try {
      entry.ws.close(1000, reason);
    } catch (_) {}
    this._devices.delete(deviceId);
  }
}

// Singleton — shared across the entire process
const connectionManager = new ConnectionManager();
export default connectionManager;
