package com.example.phoneconnect.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.example.phoneconnect.network.ConnectionState
import com.example.phoneconnect.telephony.CallLifecycle
import com.example.phoneconnect.ui.viewmodel.ConnectionViewModel

@Composable
fun HomeScreen(viewModel: ConnectionViewModel) {
    val state by viewModel.homeState.collectAsState()
    val settings by viewModel.settingsState.collectAsState()

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {

        // ── Status indicator ──────────────────────────────────────────────────
        StatusCard(connectionState = state.connectionState)

        // ── Device info ───────────────────────────────────────────────────────
        InfoCard(
            label = "Device ID",
            value = settings.deviceId.ifBlank { "Not configured" },
        )
        InfoCard(
            label = "Server",
            value = settings.serverUrl.ifBlank { "Not configured" },
        )

        // ── Call status ───────────────────────────────────────────────────────
        CallStatusCard(callLifecycle = state.callLifecycle)

        Spacer(modifier = Modifier.weight(1f))

        // ── Start / Stop ──────────────────────────────────────────────────────
        if (state.serviceRunning) {
            OutlinedButton(
                onClick = { viewModel.stopService() },
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = MaterialTheme.colorScheme.error,
                ),
            ) { Text("Disconnect") }
        } else {
            Button(
                onClick = { viewModel.startService() },
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Connect to Gateway") }
        }
    }
}

@Composable
private fun StatusCard(connectionState: ConnectionState) {
    val (dotColor, label) = when (connectionState) {
        is ConnectionState.Connected    -> Color(0xFF4CAF50) to "Connected"
        is ConnectionState.Connecting   -> Color(0xFFFFC107) to "Connecting…"
        is ConnectionState.Disconnected -> Color(0xFF9E9E9E) to "Disconnected"
        is ConnectionState.Error        -> Color(0xFFF44336) to "Error: ${connectionState.message}"
    }

    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Box(
                modifier = Modifier
                    .size(12.dp)
                    .background(dotColor, CircleShape),
            )
            Text(text = label, style = MaterialTheme.typography.bodyLarge)
        }
    }
}

@Composable
private fun InfoCard(label: String, value: String) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = value,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Medium,
            )
        }
    }
}

@Composable
private fun CallStatusCard(callLifecycle: CallLifecycle) {
    val (color, text) = when (callLifecycle) {
        is CallLifecycle.Idle          -> MaterialTheme.colorScheme.surfaceVariant to "No active call"
        is CallLifecycle.Calling       -> Color(0xFFFFC107) to "Dialling ${callLifecycle.number}…"
        is CallLifecycle.Active        -> Color(0xFF4CAF50) to "Call active: ${callLifecycle.number}"
        is CallLifecycle.Failed        -> Color(0xFFF44336) to "Call failed: ${callLifecycle.reason}"
    }

    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = color),
    ) {
        Text(
            text = text,
            modifier = Modifier.padding(16.dp),
            style = MaterialTheme.typography.bodyLarge,
        )
    }
}
