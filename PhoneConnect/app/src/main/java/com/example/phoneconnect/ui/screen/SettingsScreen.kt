package com.example.phoneconnect.ui.screen

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.example.phoneconnect.network.DiscoveryState
import com.example.phoneconnect.ui.viewmodel.ConnectionViewModel

@Composable
fun SettingsScreen(viewModel: ConnectionViewModel) {
    val saved       by viewModel.settingsState.collectAsState()
    val homeState   by viewModel.homeState.collectAsState()
    val focusManager = LocalFocusManager.current
    val scrollState  = rememberScrollState()

    // Local copies so the user can edit freely before saving
    var serverUrl by remember(saved.serverUrl) { mutableStateOf(saved.serverUrl) }
    var deviceId  by remember(saved.deviceId)  { mutableStateOf(saved.deviceId) }
    var token     by remember(saved.token)     { mutableStateOf(saved.token) }

    // Auto-fill URL when discovery resolves
    val discoveryState = homeState.discoveryState
    LaunchedEffect(discoveryState) {
        if (discoveryState is DiscoveryState.Found) {
            serverUrl = discoveryState.wsUrl
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(scrollState)
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            text = "Connection Settings",
            style = MaterialTheme.typography.titleLarge,
        )

        // ── Auto-discovery card ───────────────────────────────────────────────
        DiscoveryCard(
            state = discoveryState,
            onScanClick = {
                focusManager.clearFocus()
                viewModel.startScan()
            },
        )

        // ── Manual fields ─────────────────────────────────────────────────────
        OutlinedTextField(
            value = serverUrl,
            onValueChange = { serverUrl = it },
            label = { Text("Gateway WebSocket URL") },
            placeholder = { Text("ws://192.168.1.100:3000/ws") },
            supportingText = { Text("Auto-filled when gateway is discovered") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Next,
            ),
            keyboardActions = KeyboardActions(
                onNext = { focusManager.moveFocus(FocusDirection.Down) },
            ),
        )

        OutlinedTextField(
            value = deviceId,
            onValueChange = { deviceId = it },
            label = { Text("Device ID") },
            placeholder = { Text("android_abc123") },
            supportingText = { Text("Shown in the Home screen — used to target this device") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
            keyboardActions = KeyboardActions(
                onNext = { focusManager.moveFocus(FocusDirection.Down) },
            ),
        )

        OutlinedTextField(
            value = token,
            onValueChange = { token = it },
            label = { Text("Auth Token") },
            supportingText = { Text("Must match GATEWAY_TOKENS in the server .env") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Done,
            ),
            keyboardActions = KeyboardActions(
                onDone = { focusManager.clearFocus() },
            ),
        )

        Spacer(modifier = Modifier.height(4.dp))

        Button(
            onClick = {
                focusManager.clearFocus()
                viewModel.saveSettings(serverUrl, deviceId, token)
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = serverUrl.isNotBlank() && deviceId.isNotBlank() && token.isNotBlank(),
        ) {
            Text("Save & Reconnect")
        }
    }
}

// ── Discovery card ────────────────────────────────────────────────────────────

@Composable
private fun DiscoveryCard(
    state: DiscoveryState,
    onScanClick: () -> Unit,
) {
    val isScanning = state is DiscoveryState.Scanning

    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = when (state) {
                is DiscoveryState.Found   -> MaterialTheme.colorScheme.primaryContainer
                is DiscoveryState.Error   -> MaterialTheme.colorScheme.errorContainer
                else                      -> MaterialTheme.colorScheme.surfaceVariant
            }
        ),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                text = "Auto-Discovery",
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            Spacer(modifier = Modifier.height(8.dp))

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                when (state) {
                    is DiscoveryState.Idle -> {
                        Icon(
                            imageVector = Icons.Default.Search,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.size(20.dp),
                        )
                        Text(
                            "Find gateway automatically on your Wi-Fi",
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.weight(1f),
                        )
                    }

                    is DiscoveryState.Scanning -> {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp,
                        )
                        Text(
                            "Scanning local network for PhoneConnect gateway…",
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.weight(1f),
                        )
                    }

                    is DiscoveryState.Found -> {
                        Icon(
                            imageVector = Icons.Default.CheckCircle,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(20.dp),
                        )
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                "Gateway found!",
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            Text(
                                "${state.host}:${state.port}",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }

                    is DiscoveryState.Error -> {
                        Icon(
                            imageVector = Icons.Default.Warning,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.error,
                            modifier = Modifier.size(20.dp),
                        )
                        Text(
                            state.message,
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.weight(1f),
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            OutlinedButton(
                onClick = onScanClick,
                modifier = Modifier.fillMaxWidth(),
                enabled = !isScanning,
            ) {
                Text(if (isScanning) "Scanning…" else "Scan for Gateway")
            }
        }
    }
}

