package com.example.phoneconnect.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import com.example.phoneconnect.MainActivity
import com.example.phoneconnect.R
import com.example.phoneconnect.data.prefs.AppPreferences
import com.example.phoneconnect.network.ConnectionState
import com.example.phoneconnect.network.DiscoveryState
import com.example.phoneconnect.network.GatewayDiscovery
import com.example.phoneconnect.network.WsManager
import com.example.phoneconnect.telephony.CallManager
import com.google.gson.Gson
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch

private const val TAG             = "WsService"
private const val NOTIF_CHANNEL   = "phoneconnect_ws"
private const val NOTIF_ID        = 1001

// Intent actions used by the Activity / BootReceiver
const val ACTION_START       = "com.example.phoneconnect.ACTION_START"
const val ACTION_STOP        = "com.example.phoneconnect.ACTION_STOP"
const val ACTION_RECONFIGURE = "com.example.phoneconnect.ACTION_RECONFIGURE"
const val ACTION_SCAN        = "com.example.phoneconnect.ACTION_SCAN"

/**
 * Persistent foreground Service that owns the [WsManager] and [CallManager].
 *
 * Lifecycle:
 *  • Started by [MainActivity] or [BootReceiver] via [ACTION_START].
 *  • Stays alive as a foreground service with a persistent notification.
 *  • On [ACTION_STOP] it disconnects and stops itself.
 *  • On [ACTION_RECONFIGURE] it reloads preferences and reconnects.
 */
class WsService : Service() {

    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    lateinit var wsManager: WsManager
        private set

    lateinit var callManager: CallManager
        private set

    private lateinit var prefs: AppPreferences
    private lateinit var discovery: GatewayDiscovery

    // ── Service lifecycle ─────────────────────────────────────────────────────

    override fun onCreate() {
        super.onCreate()
        Log.d(TAG, "onCreate")
        prefs = AppPreferences(applicationContext)

        // Auto-discovery: when a gateway is found on the LAN, save the URL to prefs.
        // loadPrefsAndConnect's collectLatest will pick up the new URL and reconnect.
        discovery = GatewayDiscovery(
            context = applicationContext,
            onFound = { wsUrl ->
                serviceScope.launch(Dispatchers.IO) {
                    Log.i(TAG, "Gateway auto-discovered: $wsUrl — saving and reconnecting")
                    prefs.setServerUrl(wsUrl)
                }
            },
        )

        wsManager = WsManager(
            gson = Gson(),
            onCallCommand = { number, commandId ->
                Log.d(TAG, "CALL command received — number=$number id=$commandId")
                callManager.initiateCall(number)
            }
        )
        callManager = CallManager(applicationContext, wsManager)

        createNotificationChannel()
        // Pass the service type on API 29+ so the OS knows this is a data-sync service.
        // ServiceCompat handles the API level check internally.
        ServiceCompat.startForeground(
            this,
            NOTIF_ID,
            buildNotification("Connecting…"),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
            else
                0,
        )
        ServiceBus.setServiceRunning(true)
        observeConnectionState()
        observeCallLifecycle()
        observeDiscoveryState()
        observeLogs()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                Log.d(TAG, "Stop requested")
                discovery.stopDiscovery()
                wsManager.disconnect()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_SCAN -> {
                Log.d(TAG, "Manual scan requested")
                discovery.startDiscovery()
            }
            ACTION_RECONFIGURE, ACTION_START -> {
                Log.d(TAG, "Start/reconfigure requested")
                loadPrefsAndConnect()
            }
            else -> {
                // Restarted by OS after being killed — reconnect
                loadPrefsAndConnect()
            }
        }
        // START_STICKY so the OS restarts the service if it's killed
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        Log.d(TAG, "onDestroy")
        discovery.destroy()
        wsManager.destroy()
        callManager.destroy()
        ServiceBus.setServiceRunning(false)
        serviceScope.cancel()
        super.onDestroy()
    }

    // ── Preferences → connection ──────────────────────────────────────────────

    private fun loadPrefsAndConnect() {
        serviceScope.launch {
            combine(
                prefs.serverUrlFlow,
                prefs.deviceIdFlow,
                prefs.tokenFlow,
            ) { url, id, token -> Triple(url, id, token) }
                .collectLatest { (url, id, token) ->
                    wsManager.disconnect()
                    wsManager.configure(url, id, token)
                    wsManager.connect()

                    // Auto-Discovery: if URL is still the factory placeholder,
                    // scan the LAN for a PhoneConnect gateway.
                    if (AppPreferences.isPlaceholderUrl(url)) {
                        Log.d(TAG, "No configured URL — starting gateway discovery")
                        updateNotification("Scanning for gateway…")
                        discovery.startDiscovery()
                    } else {
                        discovery.stopDiscovery()
                    }
                }
        }
    }

    // ── Notification ──────────────────────────────────────────────────────────

    private fun observeConnectionState() {
        serviceScope.launch {
            wsManager.state.collectLatest { state ->
                ServiceBus.emitConnectionState(state)
                val text = when (state) {
                    is ConnectionState.Connected    -> "Connected to gateway"
                    is ConnectionState.Connecting   -> "Connecting…"
                    is ConnectionState.Disconnected -> "Disconnected"
                    is ConnectionState.Error        -> "Error: ${state.message}"
                }
                updateNotification(text)
            }
        }
    }

    private fun observeDiscoveryState() {
        serviceScope.launch {
            discovery.state.collectLatest { state ->
                ServiceBus.emitDiscoveryState(state)
                // Update notification text while scanning
                if (state is DiscoveryState.Scanning) {
                    updateNotification("Scanning for gateway…")
                }
            }
        }
    }

    private fun observeCallLifecycle() {
        serviceScope.launch {
            callManager.callState.collectLatest { lifecycle ->
                ServiceBus.emitCallLifecycle(lifecycle)
            }
        }
    }

    private fun observeLogs() {
        serviceScope.launch {
            wsManager.logs.collect { entry ->
                ServiceBus.emitLog(entry)
            }
        }
    }

    private fun buildNotification(text: String): Notification {
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val stopIntent = PendingIntent.getService(
            this,
            1,
            Intent(this, WsService::class.java).apply { action = ACTION_STOP },
            PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, NOTIF_CHANNEL)
            .setContentTitle("PhoneConnect")
            .setContentText(text)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentIntent(openApp)
            .addAction(0, "Stop", stopIntent)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun updateNotification(text: String) {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        nm.notify(NOTIF_ID, buildNotification(text))
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            NOTIF_CHANNEL,
            "PhoneConnect Gateway",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Shows the WebSocket gateway connection status"
        }
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        nm.createNotificationChannel(channel)
    }

    // ── Companion (start/stop helpers) ────────────────────────────────────────

    companion object {
        fun start(context: Context) {
            val intent = Intent(context, WsService::class.java).apply {
                action = ACTION_START
            }
            context.startForegroundService(intent)
        }

        fun stop(context: Context) {
            val intent = Intent(context, WsService::class.java).apply {
                action = ACTION_STOP
            }
            context.startService(intent)
        }

        fun reconfigure(context: Context) {
            val intent = Intent(context, WsService::class.java).apply {
                action = ACTION_RECONFIGURE
            }
            context.startForegroundService(intent)
        }

        fun startScan(context: Context) {
            val intent = Intent(context, WsService::class.java).apply {
                action = ACTION_SCAN
            }
            context.startService(intent)
        }
    }
}
