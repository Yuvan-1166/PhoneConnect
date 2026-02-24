package com.example.phoneconnect.network

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.net.InetAddress

private const val TAG = "GatewayDiscovery"

/**
 * The mDNS service type published by the PhoneConnect gateway.
 * bonjour-service publishes "_phoneconnect._tcp", so we query the same.
 */
private const val SERVICE_TYPE = "_phoneconnect._tcp."

// ── Discovery state ───────────────────────────────────────────────────────────

sealed class DiscoveryState {
    /** Not scanning. */
    object Idle : DiscoveryState()

    /** Actively scanning the LAN for a gateway. */
    object Scanning : DiscoveryState()

    /**
     * Gateway found and resolved.
     * @param wsUrl  Ready-to-use WebSocket URL (e.g. "ws://10.0.0.5:3000/ws")
     * @param host   Raw host IP
     * @param port   The gateway HTTP/WS port
     */
    data class Found(val wsUrl: String, val host: String, val port: Int) : DiscoveryState()

    /** A non-fatal discovery error (will not crash the app). */
    data class Error(val message: String) : DiscoveryState()
}

// ── GatewayDiscovery ──────────────────────────────────────────────────────────

/**
 * Discovers a PhoneConnect gateway on the local network using mDNS / DNS-SD
 * (the same protocol used by KDE Connect, Chromecast, AirPlay, and LocalSend).
 *
 * Algorithm:
 *  1. [startDiscovery] → NsdManager begins listening for `_phoneconnect._tcp` announcements
 *  2. First matching service found → [resolveService] → get host + port
 *  3. Build "ws://<host>:<port>/ws" URL → emit [DiscoveryState.Found] → call [onFound]
 *  4. Auto-stop discovery after first successful resolve (one gateway is enough)
 *
 * @param context   Android context (used to obtain NsdManager)
 * @param onFound   Callback invoked with the fully formed WebSocket URL once resolved
 */
class GatewayDiscovery(
    private val context: Context,
    private val onFound: (wsUrl: String) -> Unit,
) {
    private val nsdManager = context.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val _state = MutableStateFlow<DiscoveryState>(DiscoveryState.Idle)
    val state: StateFlow<DiscoveryState> = _state.asStateFlow()

    @Volatile private var discoveryListener: NsdManager.DiscoveryListener? = null
    @Volatile private var isResolving = false

    // ── Public API ────────────────────────────────────────────────────────────

    /**
     * Start scanning for a PhoneConnect gateway.
     * Safe to call multiple times — subsequent calls while already scanning are no-ops.
     */
    fun startDiscovery() {
        if (_state.value is DiscoveryState.Scanning) return
        stopDiscovery() // clean up any stale listener

        _state.value = DiscoveryState.Scanning
        Log.d(TAG, "Starting mDNS discovery for $SERVICE_TYPE")

        val listener = object : NsdManager.DiscoveryListener {
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e(TAG, "Discovery start failed (code=$errorCode)")
                _state.value = DiscoveryState.Error("Discovery unavailable (code $errorCode)")
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.w(TAG, "Discovery stop failed (code=$errorCode)")
            }

            override fun onDiscoveryStarted(serviceType: String) {
                Log.d(TAG, "Discovery active — waiting for _phoneconnect._tcp services…")
            }

            override fun onDiscoveryStopped(serviceType: String) {
                Log.d(TAG, "Discovery stopped")
            }

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                Log.d(TAG, "Service found: ${serviceInfo.serviceName}")
                // Only resolve the first hit — stop flooding NsdManager with parallel resolves
                if (isResolving) return
                isResolving = true
                resolveService(serviceInfo)
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                Log.d(TAG, "Service lost: ${serviceInfo.serviceName}")
            }
        }

        discoveryListener = listener
        nsdManager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
    }

    /**
     * Stop an in-progress scan.
     * Safe to call even if not scanning.
     */
    fun stopDiscovery() {
        val listener = discoveryListener ?: return
        discoveryListener = null
        isResolving = false

        try {
            nsdManager.stopServiceDiscovery(listener)
        } catch (e: Exception) {
            Log.w(TAG, "stopServiceDiscovery threw: ${e.message}")
        }

        if (_state.value is DiscoveryState.Scanning) {
            _state.value = DiscoveryState.Idle
        }
    }

    /** Cancel everything and release resources. Call from owning Service.onDestroy(). */
    fun destroy() {
        stopDiscovery()
        scope.cancel()
    }

    // ── Resolution ────────────────────────────────────────────────────────────

    /**
     * Resolve a discovered service to a concrete host + port.
     *
     * Uses the new [NsdManager.registerServiceInfoCallback] on API 34+ for
     * reliability, and falls back to the classic [NsdManager.resolveService]
     * on API 31–33. Both are functionally equivalent for our use-case.
     */
    private fun resolveService(serviceInfo: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // API 34+: instantaneous callback, no retry needed
            resolveServiceApi34(serviceInfo)
        } else {
            @Suppress("DEPRECATION")
            resolveServiceLegacy(serviceInfo)
        }
    }

    /**
     * API 34+ path: `registerServiceInfoCallback` resolves host + port + TXT in one shot.
     */
    @Suppress("NewApi")         // guarded by the SDK check in resolveService()
    private fun resolveServiceApi34(serviceInfo: NsdServiceInfo) {
        nsdManager.registerServiceInfoCallback(
            serviceInfo,
            { scope.launch { it.run() } },
            object : NsdManager.ServiceInfoCallback {
                override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
                    Log.e(TAG, "ServiceInfoCallback registration failed: $errorCode")
                    isResolving = false
                }

                override fun onServiceUpdated(resolvedInfo: NsdServiceInfo) {
                    val host = resolvedInfo.hostAddresses
                        .firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                        ?.hostAddress
                        ?: resolvedInfo.hostAddresses.firstOrNull()?.hostAddress
                        ?: run {
                            Log.e(TAG, "Resolved service has no usable host address")
                            isResolving = false
                            return
                        }
                    val port = resolvedInfo.port
                    handleResolved(host, port)
                    // Unregister now that we have what we need
                    try { nsdManager.unregisterServiceInfoCallback(this) } catch (_: Exception) {}
                }

                override fun onServiceLost() {
                    Log.d(TAG, "API34 resolved service lost")
                    isResolving = false
                }

                override fun onServiceInfoCallbackUnregistered() {
                    Log.d(TAG, "API34 callback unregistered")
                }
            }
        )
    }

    /**
     * API 31–33 path: classic resolve listener.
     * Deprecated on API 34 but fully functional on minSdk=31–33.
     */
    @Suppress("DEPRECATION")
    private fun resolveServiceLegacy(serviceInfo: NsdServiceInfo) {
        nsdManager.resolveService(serviceInfo, object : NsdManager.ResolveListener {
            override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                Log.e(TAG, "resolveService failed (code=$errorCode)")
                isResolving = false
            }

            override fun onServiceResolved(resolved: NsdServiceInfo) {
                val host = resolved.host
                    ?.takeIf { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                    ?.hostAddress
                    ?: resolved.host?.hostAddress
                    ?: run {
                        Log.e(TAG, "Resolved service has no usable host address")
                        isResolving = false
                        return
                    }
                handleResolved(host, resolved.port)
            }
        })
    }

    /** Common post-resolution handler — builds URL, emits state, fires callback. */
    private fun handleResolved(host: String, port: Int) {
        val wsUrl = "ws://$host:$port/ws"
        Log.i(TAG, "Gateway resolved: $wsUrl")

        _state.value = DiscoveryState.Found(wsUrl, host, port)

        // Fire callback on Main so it can safely touch Android APIs
        scope.launch(Dispatchers.Main) {
            onFound(wsUrl)
        }

        // Stop discovery — we have our gateway
        stopDiscovery()
    }
}
