import logger from "./logger.js";

const log = logger.child({ module: "auth" });

/**
 * Validates the bearer token supplied in a WebSocket upgrade request.
 *
 * Tokens are read from the GATEWAY_TOKENS env var as a comma-separated list
 * so multiple devices / callers can share the same gateway without rotating
 * a single secret:
 *
 *   GATEWAY_TOKENS=secret-android,secret-cli
 *
 * Returns { valid: boolean, reason?: string }
 */
export function verifyToken(token) {
  if (!token || typeof token !== "string" || token.trim() === "") {
    return { valid: false, reason: "No token provided" };
  }

  const allowed = (process.env.GATEWAY_TOKENS || "")
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);

  if (allowed.length === 0) {
    log.warn("GATEWAY_TOKENS is not set — all connections will be rejected");
    return { valid: false, reason: "Server not configured" };
  }

  if (!allowed.includes(token.trim())) {
    return { valid: false, reason: "Invalid token" };
  }

  return { valid: true };
}

/**
 * Express middleware that checks the Authorization header.
 * Usage:  router.post('/call', requireAuth, handler)
 */
export function requireAuth(req, res, next) {
  const header = req.headers["authorization"] || "";
  const token = header.startsWith("Bearer ") ? header.slice(7) : header;

  const { valid, reason } = verifyToken(token);
  if (!valid) {
    log.warn({ ip: req.ip, reason }, "REST auth failed");
    return res.status(401).json({ error: "Unauthorized", reason });
  }
  next();
}
