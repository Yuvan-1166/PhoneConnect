package com.example.phoneconnect.network

import android.util.Log
import com.example.phoneconnect.data.model.AckMessage
import com.example.phoneconnect.data.model.AuthMessage
import com.example.phoneconnect.data.model.InboundMessage
import com.example.phoneconnect.data.model.PongMessage
import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import kotlin.math.min
import kotlin.math.pow

private const val TAG = "WsManager"

/** How long the server may silently idle before we consider it dead (ms). */
private const val PONG_TIMEOUT_MS       = 30_000L
/** Interval between client-initiated pings when the server is silent (ms). */
private const val PING_INTERVAL_MS      = 25_000L
private const val MAX_RECONNECT_DELAY_MS = 60_000L
private const val BASE_RECONNECT_DELAY_MS = 1_000L

// ─── Connection state ─────────────────────────────────────────────────────────

sealed class ConnectionState {
    object Disconnected : ConnectionState()
    object Connecting   : ConnectionState()
    object Connected    : ConnectionState()
    data class Error(val message: String) : ConnectionState()
}

// ─── WsManager ────────────────────────────────────────────────────────────────

/**
 * Manages the lifecycle of one persistent OkHttp WebSocket connection.
 *
 * Features:
 *  • Exponential back-off reconnection (capped at 60 s)
 *  • AUTH handshake on open
 *  • Ping/Pong heartbeat loop
 *  • Duplicate-command deduplication via a bounded LRU set
 *  • All callbacks marshalled to a structured coroutine scope
 */
class WsManager(
    private val gson: Gson,
    private val onCallCommand: (number: String, commandId: String) -> Unit,
) {
    // ── State exposed to UI ───────────────────────────────────────────────────

    private val _state = MutableStateFlow<ConnectionState>(ConnectionState.Disconnected)
    val state: StateFlow<ConnectionState> = _state.asStateFlow()

    private val _logs = MutableSharedFlow<String>(replay = 50, extraBufferCapacity = 100)
    val logs: SharedFlow<String> = _logs.asSharedFlow()

    // ── Internals ─────────────────────────────────────────────────────────────

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val httpClient: OkHttpClient = OkHttpClient.Builder()
        .pingInterval(PING_INTERVAL_MS, TimeUnit.MILLISECONDS)
        .readTimeout(PONG_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        .build()

    @Volatile private var socket: WebSocket? = null
    @Volatile private var shouldRun = false

    private var serverUrl = ""
    private var deviceId  = ""
    private var token     = ""

    private var reconnectAttempt = 0
    private var reconnectJob: Job? = null
    private var pingJob: Job? = null

    /** Bounded dedup set: tracks the last 200 processed command IDs. */
    private val processedIds: MutableSet<String> = object : LinkedHashMap<String, Boolean>(256, 0.75f, true) {
        override fun removeEldestEntry(eldest: Map.Entry<String, Boolean>) = size > 200
    }.keys

    // ── Public API ────────────────────────────────────────────────────────────

    fun configure(url: String, id: String, tok: String) {
        serverUrl = url
        deviceId  = id
        token     = tok
    }

    /** Begin connecting (or reconnecting). */
    fun connect() {
        shouldRun = true
        reconnectAttempt = 0
        openSocket()
    }

    /** Permanently disconnect and stop reconnection attempts. */
    fun disconnect() {
        shouldRun = false
        reconnectJob?.cancel()
        pingJob?.cancel()
        socket?.close(1000, "User disconnected")
        socket = null
        _state.value = ConnectionState.Disconnected
        log("Disconnected by user")
    }

    /** Send an arbitrary JSON-serialisable object. Returns false if not connected. */
    fun sendMessage(payload: Any): Boolean {
        val ws = socket ?: return false
        val json = gson.toJson(payload)
        log("→ $json")
        return ws.send(json)
    }

    /** Clean up the coroutine scope. Call when the owning Service is destroyed. */
    fun destroy() {
        disconnect()
        scope.cancel()
    }

    // ── Socket lifecycle ──────────────────────────────────────────────────────

    private fun openSocket() {
        if (serverUrl.isBlank()) {
            log("Server URL is not configured — aborting connection")
            _state.value = ConnectionState.Error("Server URL not set")
            return
        }
        _state.value = ConnectionState.Connecting
        log("Connecting to $serverUrl (attempt ${reconnectAttempt + 1})")

        val request = Request.Builder().url(serverUrl).build()
        socket = httpClient.newWebSocket(request, WsListener())
    }

    private inner class WsListener : WebSocketListener() {

        override fun onOpen(webSocket: WebSocket, response: Response) {
            log("Socket opened")
            reconnectAttempt = 0
            _state.value = ConnectionState.Connected

            // Authenticate immediately
            val auth = AuthMessage(deviceId = deviceId, token = token)
            webSocket.send(gson.toJson(auth))
            log("→ AUTH deviceId=$deviceId")

            startPingLoop()
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            log("← $text")
            handleInbound(text)
        }

        override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
            // Binary frames are not used by this protocol — ignore silently
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            log("Socket closing: $code $reason")
            webSocket.close(1000, null)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            log("Socket closed: $code $reason")
            pingJob?.cancel()
            _state.value = ConnectionState.Disconnected
            scheduleReconnect()
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            val msg = t.message ?: "Unknown error"
            log("Socket error: $msg")
            pingJob?.cancel()
            _state.value = ConnectionState.Error(msg)
            scheduleReconnect()
        }
    }

    // ── Message handling ──────────────────────────────────────────────────────

    private fun handleInbound(raw: String) {
        try {
            val obj: JsonObject = JsonParser.parseString(raw).asJsonObject
            val type = obj.get("type")?.asString ?: return

            when (type.uppercase()) {
                "CALL" -> {
                    val number    = obj.get("number")?.asString ?: return
                    val commandId = obj.get("id")?.asString ?: ""

                    if (commandId.isNotBlank() && processedIds.contains(commandId)) {
                        log("Duplicate command $commandId — ignored")
                        return
                    }
                    if (commandId.isNotBlank()) processedIds.add(commandId)

                    // Validate phone number (E.164 basic check)
                    if (!isValidPhone(number)) {
                        log("Invalid phone number: $number")
                        return
                    }

                    // ACK before triggering the call
                    if (commandId.isNotBlank()) sendMessage(AckMessage(id = commandId))

                    onCallCommand(number, commandId)
                }

                "PING" -> {
                    sendMessage(PongMessage())
                }

                else -> log("Unhandled message type: $type")
            }
        } catch (e: Exception) {
            log("Parse error: ${e.message}")
        }
    }

    // ── Heartbeat ─────────────────────────────────────────────────────────────

    private fun startPingLoop() {
        pingJob?.cancel()
        pingJob = scope.launch {
            // OkHttp handles WS-level ping/pong automatically when pingInterval is set.
            // This loop is a secondary application-level keepalive for servers that
            // use the custom PING JSON message instead of WS frames.
            while (true) {
                delay(PING_INTERVAL_MS)
                if (_state.value is ConnectionState.Connected) {
                    // sendMessage(PingMessage()) – only needed if server expects JSON pings
                    // OkHttp native ping is sufficient; no-op here.
                }
            }
        }
    }

    // ── Reconnection ──────────────────────────────────────────────────────────

    private fun scheduleReconnect() {
        if (!shouldRun) return

        val delayMs = min(
            BASE_RECONNECT_DELAY_MS * 2.0.pow(reconnectAttempt).toLong(),
            MAX_RECONNECT_DELAY_MS
        )
        reconnectAttempt++

        log("Reconnecting in ${delayMs}ms (attempt $reconnectAttempt)")
        reconnectJob?.cancel()
        reconnectJob = scope.launch {
            delay(delayMs)
            if (shouldRun) openSocket()
        }
    }

    // ── Utilities ─────────────────────────────────────────────────────────────

    /**
     * Basic E.164 validation: optional +, 7–15 digits.
     * For production deploy libphonenumber for full validation.
     */
    private fun isValidPhone(number: String): Boolean =
        number.matches(Regex("^\\+?[1-9]\\d{6,14}$"))

    private fun log(message: String) {
        val timestamped = "[${System.currentTimeMillis()}] $message"
        Log.d(TAG, timestamped)
        scope.launch { _logs.emit(timestamped) }
    }
}
