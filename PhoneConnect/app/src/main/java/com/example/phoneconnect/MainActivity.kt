package com.example.phoneconnect

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.List
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import com.example.phoneconnect.service.WsService
import com.example.phoneconnect.ui.screen.HomeScreen
import com.example.phoneconnect.ui.screen.LogsScreen
import com.example.phoneconnect.ui.screen.SettingsScreen
import com.example.phoneconnect.ui.theme.PhoneConnectTheme
import com.example.phoneconnect.ui.viewmodel.ConnectionViewModel

// ─── Navigation routes ────────────────────────────────────────────────────────
private const val ROUTE_HOME     = "home"
private const val ROUTE_LOGS     = "logs"
private const val ROUTE_SETTINGS = "settings"

class MainActivity : ComponentActivity() {

    private val viewModel: ConnectionViewModel by viewModels()

    // ── Runtime permission launcher ───────────────────────────────────────────

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { results ->
        val denied = results.filterValues { !it }.keys
        if (denied.isEmpty()) {
            // All granted — start the gateway service
            viewModel.startService()
        } else {
            Toast.makeText(
                this,
                "Required permissions denied: ${denied.joinToString()}",
                Toast.LENGTH_LONG,
            ).show()
        }
    }

    // ── Activity lifecycle ────────────────────────────────────────────────────

    override fun onCreate(savedInstanceState: Bundle?) {
        installSplashScreen()
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Request permissions on first launch
        ensurePermissions()

        setContent {
            PhoneConnectTheme {
                val navController = rememberNavController()

                val bottomItems = listOf(
                    Triple(ROUTE_HOME,     "Home",     Icons.Filled.Home),
                    Triple(ROUTE_LOGS,     "Logs",     Icons.Filled.List),
                    Triple(ROUTE_SETTINGS, "Settings", Icons.Filled.Settings),
                )

                Scaffold(
                    bottomBar = {
                        val navBackStack by navController.currentBackStackEntryAsState()
                        val currentDest = navBackStack?.destination

                        NavigationBar {
                            bottomItems.forEach { (route, label, icon) ->
                                NavigationBarItem(
                                    selected = currentDest?.hierarchy?.any { it.route == route } == true,
                                    onClick = {
                                        navController.navigate(route) {
                                            popUpTo(navController.graph.findStartDestination().id) {
                                                saveState = true
                                            }
                                            launchSingleTop = true
                                            restoreState    = true
                                        }
                                    },
                                    icon  = { Icon(icon, contentDescription = label) },
                                    label = { Text(label) },
                                )
                            }
                        }
                    },
                ) { innerPadding ->
                    NavHost(
                        navController    = navController,
                        startDestination = ROUTE_HOME,
                        modifier         = Modifier.padding(innerPadding),
                    ) {
                        composable(ROUTE_HOME)     { HomeScreen(viewModel) }
                        composable(ROUTE_LOGS)     { LogsScreen(viewModel) }
                        composable(ROUTE_SETTINGS) { SettingsScreen(viewModel) }
                    }
                }
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private fun ensurePermissions() {
        val required = buildList {
            add(Manifest.permission.CALL_PHONE)
            add(Manifest.permission.READ_PHONE_STATE)
            // POST_NOTIFICATIONS only required on Android 13+
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
                add(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
        val missing = required.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isEmpty()) {
            viewModel.startService()
        } else {
            permissionLauncher.launch(missing.toTypedArray())
        }
    }
}
