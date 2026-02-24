import pino from "pino";

/**
 * Structured logger backed by pino.
 *
 * In development (NODE_ENV=development) logs are pretty-printed with colours.
 * In production logs are emitted as newline-delimited JSON for log aggregators.
 *
 * Child loggers are created per-module so every line carries a `module` field:
 *   logger.child({ module: 'connectionManager' })
 */
const isDev = process.env.NODE_ENV === "development";

const logger = pino({
  level: process.env.LOG_LEVEL || "info",
  ...(isDev && {
    transport: {
      target: "pino-pretty",
      options: {
        colorize: true,
        translateTime: "SYS:HH:MM:ss",
        ignore: "pid,hostname",
      },
    },
  }),
  // Redact sensitive fields from all log lines
  redact: {
    paths: ["token", "req.headers.authorization", "*.token"],
    censor: "[REDACTED]",
  },
  base: { service: "phoneconnect-gateway" },
});

export default logger;
