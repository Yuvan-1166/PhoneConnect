import { Router } from "express";
import { body, validationResult } from "express-validator";
import connectionManager from "../connectionManager.js";
import { buildCallCommand } from "../messageHandler.js";
import { requireAuth } from "../auth.js";
import { apiLimiter } from "../rateLimiter.js";
import logger from "../logger.js";

const log = logger.child({ module: "routes/call" });
const router = Router();

// ── Validation rules ──────────────────────────────────────────────────────────

const callValidation = [
  body("deviceId")
    .isString()
    .trim()
    .notEmpty()
    .withMessage("deviceId is required"),

  body("number")
    .isString()
    .trim()
    .notEmpty()
    .matches(/^\+?[1-9]\d{6,14}$/)
    .withMessage("number must be a valid E.164 phone number"),
];

// ── POST /call ────────────────────────────────────────────────────────────────

/**
 * Trigger a call on a specific connected Android device.
 *
 * Request:
 *   POST /call
 *   Authorization: Bearer <token>
 *   { "deviceId": "android_abc123", "number": "+919876543210" }
 *
 * Responses:
 *   200  { ok: true,  commandId, deviceId }
 *   400  { error: "Validation failed", details: [...] }
 *   401  { error: "Unauthorized" }
 *   404  { error: "Device not connected", deviceId }
 *   429  { error: "Too many requests" }
 *   500  { error: "Internal server error" }
 */
router.post(
  "/call",
  apiLimiter,
  requireAuth,
  callValidation,
  (req, res) => {
    // Validation errors
    const errors = validationResult(req);
    if (!errors.isEmpty()) {
      return res.status(400).json({
        error: "Validation failed",
        details: errors.array().map((e) => ({ field: e.path, msg: e.msg })),
      });
    }

    const { deviceId, number } = req.body;

    if (!connectionManager.isConnected(deviceId)) {
      log.warn({ deviceId }, "Call requested for offline device");
      return res.status(404).json({
        error: "Device not connected",
        deviceId,
        connectedDevices: connectionManager.connectedDevices(),
      });
    }

    const command = buildCallCommand(number);
    const sent = connectionManager.sendTo(deviceId, command);

    if (!sent) {
      log.error({ deviceId }, "sendTo returned false after isConnected check");
      return res.status(500).json({ error: "Internal server error" });
    }

    log.info({ deviceId, number, commandId: command.id }, "CALL command dispatched");
    return res.status(200).json({
      ok: true,
      commandId: command.id,
      deviceId,
    });
  }
);

// ── GET /devices ──────────────────────────────────────────────────────────────

/**
 * List all currently connected devices (useful for CLI / debugging).
 *
 * GET /devices
 * Authorization: Bearer <token>
 */
router.get("/devices", requireAuth, (_req, res) => {
  res.json(connectionManager.stats());
});

// ── GET /health ───────────────────────────────────────────────────────────────

/**
 * Unauthenticated health check — used by load balancers / monitoring.
 */
router.get("/health", (_req, res) => {
  res.json({
    status: "ok",
    uptime: process.uptime(),
    connectedDevices: connectionManager.connectedDevices().length,
    timestamp: new Date().toISOString(),
  });
});

export default router;
