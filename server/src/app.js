import "dotenv/config";
import http from "http";
import express from "express";
import { WebSocketServer } from "ws";
import { v4 as uuidv4 } from "uuid";
import logger from "./logger.js";
import connectionManager from "./connectionManager.js";
import { handleMessage } from "./messageHandler.js";
import apiRouter from "./routes/call.js";

const log = logger.child({ module: "app" });

// ── Express app ───────────────────────────────────────────────────────────────

const app = express();
app.use(express.json());

// Attach all REST routes
app.use("/", apiRouter);

// ── HTTP server ───────────────────────────────────────────────────────────────

const PORT = parseInt(process.env.PORT || "3000", 10);
const server = http.createServer(app);

// ── WebSocket server ──────────────────────────────────────────────────────────

const wss = new WebSocketServer({
  server,
  path: "/ws",
});

wss.on("connection", (ws, req) => {
  const connId = uuidv4().slice(0, 8);
  const ip = req.headers["x-forwarded-for"] || req.socket.remoteAddress;

  log.info({ connId, ip }, "New WebSocket connection");

  // Per-connection state — mutated by messageHandler on AUTH
  const state = {
    authenticated: false,
    deviceId: null,
  };

  ws.on("message", (data) => {
    const raw = data.toString();
    handleMessage(ws, raw, state);
  });

  ws.on("close", (code, reason) => {
    if (!state.authenticated) {
      log.info({ connId, ip, code }, "Unauthenticated connection closed");
    }
    // Authenticated close is handled by connectionManager
  });

  ws.on("error", (err) => {
    log.error({ connId, ip, err: err.message }, "WebSocket error");
  });
});

wss.on("error", (err) => {
  log.error({ err: err.message }, "WebSocketServer error");
});

// ── Graceful shutdown ─────────────────────────────────────────────────────────

function shutdown(signal) {
  log.info({ signal }, "Shutting down gracefully…");
  wss.close(() => {
    server.close(() => {
      log.info("Server closed");
      process.exit(0);
    });
  });

  // Force-exit after 10 s if graceful shutdown stalls
  setTimeout(() => {
    log.error("Forced exit after timeout");
    process.exit(1);
  }, 10_000);
}

process.on("SIGTERM", () => shutdown("SIGTERM"));
process.on("SIGINT",  () => shutdown("SIGINT"));

// ── Start ─────────────────────────────────────────────────────────────────────

server.listen(PORT, () => {
  log.info(
    {
      port: PORT,
      wsPath: "/ws",
      env: process.env.NODE_ENV || "production",
    },
    "PhoneConnect gateway started"
  );
});

