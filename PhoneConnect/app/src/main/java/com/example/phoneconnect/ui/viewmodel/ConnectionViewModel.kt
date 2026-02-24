package com.example.phoneconnect.ui.viewmodel

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.example.phoneconnect.data.prefs.AppPreferences
import com.example.phoneconnect.network.ConnectionState
import com.example.phoneconnect.network.DiscoveryState
import com.example.phoneconnect.service.ServiceBus
import com.example.phoneconnect.service.WsService
import com.example.phoneconnect.telephony.CallLifecycle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class SettingsUiState(
    val serverUrl: String = "",
    val deviceId:  String = "",
    val token:     String = "",
)

data class HomeUiState(
    val connectionState: ConnectionState = ConnectionState.Disconnected,
    val callLifecycle:   CallLifecycle   = CallLifecycle.Idle,
    val logs:            List<String>    = emptyList(),
    val serviceRunning:  Boolean         = false,
    val discoveryState:  DiscoveryState  = DiscoveryState.Idle,
)

/**
 * Shared ViewModel: bridges [ServiceBus] flows + [AppPreferences] to all Compose screens.
 */
class ConnectionViewModel(application: Application) : AndroidViewModel(application) {

    private val prefs = AppPreferences(application)

    // ── Settings state ────────────────────────────────────────────────────────

    val settingsState: StateFlow<SettingsUiState> = run {
        val flow = MutableStateFlow(SettingsUiState())
        viewModelScope.launch {
            prefs.serverUrlFlow.collect { url -> flow.update { it.copy(serverUrl = url) } }
        }
        viewModelScope.launch {
            prefs.deviceIdFlow.collect { id -> flow.update { it.copy(deviceId = id) } }
        }
        viewModelScope.launch {
            prefs.tokenFlow.collect { tok -> flow.update { it.copy(token = tok) } }
        }
        flow.asStateFlow()
    }

    // ── Live home state (from ServiceBus) ─────────────────────────────────────

    private val _homeState = MutableStateFlow(HomeUiState())
    val homeState: StateFlow<HomeUiState> = _homeState.asStateFlow()

    init {
        viewModelScope.launch { prefs.ensureDeviceId() }

        // Observe connection state
        viewModelScope.launch {
            ServiceBus.connectionState.collect { state ->
                _homeState.update { it.copy(connectionState = state) }
            }
        }
        // Observe call lifecycle
        viewModelScope.launch {
            ServiceBus.callLifecycle.collect { lifecycle ->
                _homeState.update { it.copy(callLifecycle = lifecycle) }
            }
        }
        // Observe service running flag
        viewModelScope.launch {
            ServiceBus.serviceRunning.collect { running ->
                _homeState.update { it.copy(serviceRunning = running) }
            }
        }
        // Observe gateway discovery state
        viewModelScope.launch {
            ServiceBus.discoveryState.collect { discovery ->
                _homeState.update { it.copy(discoveryState = discovery) }
            }
        }
        // Collect incremental logs (keep last 500 entries to avoid OOM)
        viewModelScope.launch {
            ServiceBus.logs.collect { entry ->
                _homeState.update { state ->
                    val updated = (state.logs + entry).takeLast(500)
                    state.copy(logs = updated)
                }
            }
        }
    }

    // ── Public commands ───────────────────────────────────────────────────────

    fun startService() {
        WsService.start(getApplication())
    }

    fun stopService() {
        WsService.stop(getApplication())
    }

    fun saveSettings(url: String, deviceId: String, token: String) {
        viewModelScope.launch {
            prefs.setServerUrl(url)
            prefs.setDeviceId(deviceId)
            prefs.setToken(token)
            WsService.reconfigure(getApplication())
        }
    }

    fun startScan() {
        WsService.startScan(getApplication())
    }
}
