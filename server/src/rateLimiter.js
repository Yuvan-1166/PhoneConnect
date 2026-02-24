import rateLimit from "express-rate-limit";
import logger from "./logger.js";

const log = logger.child({ module: "rateLimiter" });

/**
 * Rate limiter for the POST /call REST endpoint.
 *
 * Defaults (overridable via env):
 *   RATE_LIMIT_WINDOW_MS  = 60_000   (1 minute window)
 *   RATE_LIMIT_MAX        = 30       (30 requests per window per IP)
 */
const windowMs = parseInt(process.env.RATE_LIMIT_WINDOW_MS || "60000", 10);
const max = parseInt(process.env.RATE_LIMIT_MAX || "30", 10);

export const apiLimiter = rateLimit({
  windowMs,
  max,
  standardHeaders: true,   // Return RateLimit-* headers
  legacyHeaders: false,
  handler(req, res) {
    log.warn({ ip: req.ip }, "Rate limit exceeded");
    res.status(429).json({
      error: "Too many requests",
      retryAfter: Math.ceil(windowMs / 1000),
    });
  },
});
