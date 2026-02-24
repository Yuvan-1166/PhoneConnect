package com.example.phoneconnect.service

import com.example.phoneconnect.network.ConnectionState
import com.example.phoneconnect.network.DiscoveryState
import com.example.phoneconnect.telephony.CallLifecycle
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Process-scoped singleton bus that lets the [WsService] publish its state
 * to the ViewModel without a Binder/AIDL binding.
 *
 * The ViewModel collects these flows; the Service emits into them.
 * Cleared automatically when the process dies.
 */
object ServiceBus {

    private val _connectionState = MutableStateFlow<ConnectionState>(ConnectionState.Disconnected)
    val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    private val _callLifecycle = MutableStateFlow<CallLifecycle>(CallLifecycle.Idle)
    val callLifecycle: StateFlow<CallLifecycle> = _callLifecycle.asStateFlow()

    private val _logs = MutableSharedFlow<String>(replay = 200, extraBufferCapacity = 500)
    val logs: SharedFlow<String> = _logs.asSharedFlow()

    private val _serviceRunning = MutableStateFlow(false)
    val serviceRunning: StateFlow<Boolean> = _serviceRunning.asStateFlow()

    private val _discoveryState = MutableStateFlow<DiscoveryState>(DiscoveryState.Idle)
    val discoveryState: StateFlow<DiscoveryState> = _discoveryState.asStateFlow()

    // ── Emit helpers (called by WsService) ───────────────────────────────────

    fun emitConnectionState(state: ConnectionState)  { _connectionState.value = state }
    fun emitCallLifecycle(lifecycle: CallLifecycle)  { _callLifecycle.value = lifecycle }
    suspend fun emitLog(entry: String)               { _logs.emit(entry) }
    fun setServiceRunning(running: Boolean)          { _serviceRunning.value = running }
    fun emitDiscoveryState(state: DiscoveryState)    { _discoveryState.value = state }
}
