package com.example.phoneconnect.receiver

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import com.example.phoneconnect.service.WsService

private const val TAG = "BootReceiver"

/**
 * Starts the gateway foreground service after the device boots.
 * Requires RECEIVE_BOOT_COMPLETED permission.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) {
            Log.d(TAG, "Boot completed — starting WsService")
            WsService.start(context)
        }
    }
}
