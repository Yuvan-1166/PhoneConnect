import { v4 as uuidv4 } from "uuid";
import { verifyToken } from "./auth.js";
import connectionManager from "./connectionManager.js";
import logger from "./logger.js";

const log = logger.child({ module: "messageHandler" });

/**
 * Called for every raw text message received from a device WebSocket.
 *
 * Expected inbound message types (device → gateway):
 *
 *   AUTH     { type, deviceId, token }          — first message after connect
 *   STATUS   { type, state, number? }           — call lifecycle update
 *   ACK      { type, id }                       — command acknowledgement
 *   PONG     { type }                           — response to JSON PING
 *
 * @param {WebSocket} ws          - The raw socket that sent the message
 * @param {string}    rawMessage  - Raw JSON string
 * @param {object}    state       - Mutable per-connection state object
 *                                  { authenticated: bool, deviceId: string|null }
 */
export function handleMessage(ws, rawMessage, state) {
  let msg;
  try {
    msg = JSON.parse(rawMessage);
  } catch {
    log.warn("Received non-JSON message — dropping");
    ws.send(JSON.stringify({ type: "ERROR", reason: "Invalid JSON" }));
    return;
  }

  const type = (msg.type || "").toUpperCase();

  // ── AUTH must be the first message ────────────────────────────────────────
  if (!state.authenticated) {
    if (type !== "AUTH") {
      log.warn({ type }, "Unauthenticated message before AUTH — closing");
      ws.close(1008, "AUTH required");
      return;
    }
    return handleAuth(ws, msg, state);
  }

  // ── Authenticated message routing ─────────────────────────────────────────
  switch (type) {
    case "STATUS":
      return handleStatus(msg, state);
    case "ACK":
      return handleAck(msg, state);
    case "PONG":
      // JSON-level pong (WS-level pong is handled by OkHttp natively)
      log.debug({ deviceId: state.deviceId }, "JSON PONG received");
      break;
    default:
      log.warn({ deviceId: state.deviceId, type }, "Unknown message type");
      ws.send(JSON.stringify({ type: "ERROR", reason: `Unknown type: ${type}` }));
  }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

function handleAuth(ws, msg, state) {
  const { deviceId, token } = msg;

  if (!deviceId || typeof deviceId !== "string" || deviceId.trim() === "") {
    ws.close(1008, "Missing deviceId");
    return;
  }

  const { valid, reason } = verifyToken(token);
  if (!valid) {
    log.warn({ deviceId, reason }, "AUTH failed");
    ws.send(JSON.stringify({ type: "AUTH_FAILED", reason }));
    ws.close(1008, "Unauthorized");
    return;
  }

  state.authenticated = true;
  state.deviceId = deviceId.trim();

  connectionManager.register(state.deviceId, ws);
  ws.send(JSON.stringify({ type: "AUTH_OK", deviceId: state.deviceId }));
  log.info({ deviceId: state.deviceId }, "AUTH OK");
}

function handleStatus(msg, state) {
  const VALID_STATES = ["CALL_STARTED", "CALL_ENDED", "CALL_FAILED"];
  const callState = (msg.state || "").toUpperCase();

  if (!VALID_STATES.includes(callState)) {
    log.warn({ deviceId: state.deviceId, callState }, "Unknown call state");
    return;
  }

  log.info(
    { deviceId: state.deviceId, callState, number: msg.number ?? null },
    "Call status update"
  );

  // Future: emit to REST status SSE stream or a webhook here
}

function handleAck(msg, state) {
  const id = msg.id || "";
  log.debug({ deviceId: state.deviceId, commandId: id }, "ACK received");
  // Future: resolve a pending-command promise here for guaranteed delivery
}

// ── Outbound helpers (gateway → device) ───────────────────────────────────────

/**
 * Build a CALL command payload with a unique ID for dedup on the device.
 */
export function buildCallCommand(number) {
  return {
    type: "CALL",
    number,
    id: uuidv4(),
  };
}
