package com.example.phoneconnect.data.model

import com.google.gson.annotations.SerializedName

// ─── Sealed hierarchy for inbound gateway messages ───────────────────────────

sealed class InboundMessage {
    /** Server → App: initiate a cellular call */
    data class CallCommand(
        @SerializedName("type") val type: String = "CALL",
        @SerializedName("number") val number: String,
        @SerializedName("id") val id: String = "",   // unique command id for dedup
    ) : InboundMessage()

    /** Server → App: ping keepalive */
    data class Ping(
        @SerializedName("type") val type: String = "PING",
    ) : InboundMessage()

    /** Fallback for unknown types */
    data class Unknown(val raw: String) : InboundMessage()
}

// ─── Outbound messages (App → Server) ────────────────────────────────────────

/**
 * Authentication handshake sent immediately after WS opens.
 */
data class AuthMessage(
    @SerializedName("type") val type: String = "AUTH",
    @SerializedName("deviceId") val deviceId: String,
    @SerializedName("token") val token: String,
)

/**
 * Acknowledgement of a received command.
 */
data class AckMessage(
    @SerializedName("type") val type: String = "ACK",
    @SerializedName("id") val id: String,
)

/**
 * Call lifecycle status report.
 */
data class StatusMessage(
    @SerializedName("type") val type: String = "STATUS",
    @SerializedName("state") val state: String,     // CALL_STARTED | CALL_ENDED | CALL_FAILED
    @SerializedName("number") val number: String? = null,
)

/**
 * Pong response to server ping.
 */
data class PongMessage(
    @SerializedName("type") val type: String = "PONG",
)

// ─── Call state enum ─────────────────────────────────────────────────────────

enum class CallState(val raw: String) {
    CALL_STARTED("CALL_STARTED"),
    CALL_ENDED("CALL_ENDED"),
    CALL_FAILED("CALL_FAILED"),
}
