package com.example.phoneconnect.telephony

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.telephony.PhoneStateListener
import android.telephony.TelephonyManager
import android.util.Log
import com.example.phoneconnect.data.model.CallState
import com.example.phoneconnect.data.model.StatusMessage
import com.example.phoneconnect.network.WsManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

private const val TAG = "CallManager"

/**
 * Manages outgoing call initiation and monitors call lifecycle via [TelephonyManager].
 *
 * Call flow:
 *   1. [initiateCall] fires ACTION_CALL intent.
 *   2. [CallStateObserver] (PhoneStateListener) watches for OFFHOOK → reports CALL_STARTED.
 *   3. When state returns to IDLE after OFFHOOK → reports CALL_ENDED.
 *   4. Any intent failure → reports CALL_FAILED.
 */
class CallManager(
    private val context: Context,
    private val wsManager: WsManager,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val telephonyManager =
        context.getSystemService(Context.TELEPHONY_SERVICE) as TelephonyManager

    private val _callState = MutableStateFlow<CallLifecycle>(CallLifecycle.Idle)
    val callState: StateFlow<CallLifecycle> = _callState.asStateFlow()

    private val phoneStateListener = CallStateObserver()
    private var activeNumber: String? = null

    init {
        @Suppress("DEPRECATION")
        telephonyManager.listen(phoneStateListener, PhoneStateListener.LISTEN_CALL_STATE)
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /**
     * Initiates a cellular call via ACTION_CALL intent.
     * Requires CALL_PHONE permission to be granted at runtime before this call.
     */
    fun initiateCall(number: String) {
        activeNumber = number
        Log.d(TAG, "Initiating call to $number")
        try {
            val intent = Intent(Intent.ACTION_CALL, Uri.parse("tel:$number")).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
            _callState.value = CallLifecycle.Calling(number)
        } catch (e: SecurityException) {
            Log.e(TAG, "Permission denied for CALL_PHONE: ${e.message}")
            reportStatus(CallState.CALL_FAILED, number)
            _callState.value = CallLifecycle.Failed("Permission denied")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to initiate call: ${e.message}")
            reportStatus(CallState.CALL_FAILED, number)
            _callState.value = CallLifecycle.Failed(e.message ?: "Unknown error")
        }
    }

    /**
     * Unregister the phone state listener.  Call from Service#onDestroy().
     */
    fun destroy() {
        @Suppress("DEPRECATION")
        telephonyManager.listen(phoneStateListener, PhoneStateListener.LISTEN_NONE)
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private fun reportStatus(state: CallState, number: String? = activeNumber) {
        val msg = StatusMessage(state = state.raw, number = number)
        val sent = wsManager.sendMessage(msg)
        Log.d(TAG, "Status reported: ${state.raw} (sent=$sent)")
        scope.launch {
            _callState.value = when (state) {
                CallState.CALL_STARTED -> CallLifecycle.Active(number ?: "")
                CallState.CALL_ENDED   -> CallLifecycle.Idle
                CallState.CALL_FAILED  -> CallLifecycle.Failed("Call failed")
            }
        }
    }

    // ── PhoneStateListener ────────────────────────────────────────────────────

    @Suppress("DEPRECATION")
    private inner class CallStateObserver : PhoneStateListener() {
        private var wasOffHook = false

        override fun onCallStateChanged(state: Int, phoneNumber: String?) {
            when (state) {
                TelephonyManager.CALL_STATE_OFFHOOK -> {
                    if (!wasOffHook) {
                        wasOffHook = true
                        Log.d(TAG, "Call state: OFFHOOK")
                        reportStatus(CallState.CALL_STARTED)
                    }
                }
                TelephonyManager.CALL_STATE_IDLE -> {
                    if (wasOffHook) {
                        wasOffHook = false
                        activeNumber = null
                        Log.d(TAG, "Call state: IDLE (after OFFHOOK = call ended)")
                        reportStatus(CallState.CALL_ENDED, null)
                    }
                }
                TelephonyManager.CALL_STATE_RINGING -> {
                    // Incoming call while we placed an outgoing one — edge case, ignore
                }
            }
        }
    }
}

// ─── Call lifecycle UI state ──────────────────────────────────────────────────

sealed class CallLifecycle {
    object Idle                          : CallLifecycle()
    data class Calling(val number: String) : CallLifecycle()
    data class Active(val number: String)  : CallLifecycle()
    data class Failed(val reason: String)  : CallLifecycle()
}
