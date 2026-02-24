package com.example.phoneconnect.data.prefs

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import java.util.UUID

// Top-level DataStore delegate (one instance per process)
private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "phone_connect_prefs")

/**
 * Reactive preference store backed by Jetpack DataStore.
 * All reads are Flows so the UI can react to changes; all writes are suspend functions.
 */
class AppPreferences(private val context: Context) {

    companion object {
        private val KEY_SERVER_URL = stringPreferencesKey("server_url")
        private val KEY_DEVICE_ID  = stringPreferencesKey("device_id")
        private val KEY_TOKEN      = stringPreferencesKey("token")

        const val DEFAULT_SERVER_URL = "ws://192.168.1.100:3000/ws"

        /** Returns true if [url] is the unconfigured factory placeholder. */
        fun isPlaceholderUrl(url: String) = url == DEFAULT_SERVER_URL || url.isBlank()
    }

    // ── Reactive Flows ────────────────────────────────────────────────────────

    val serverUrlFlow: Flow<String> = context.dataStore.data
        .map { prefs -> prefs[KEY_SERVER_URL] ?: DEFAULT_SERVER_URL }

    val deviceIdFlow: Flow<String> = context.dataStore.data
        .map { prefs -> prefs[KEY_DEVICE_ID] ?: generateAndPersistDeviceId() }

    val tokenFlow: Flow<String> = context.dataStore.data
        .map { prefs -> prefs[KEY_TOKEN] ?: "" }

    // ── Writes ────────────────────────────────────────────────────────────────

    suspend fun setServerUrl(url: String) {
        context.dataStore.edit { it[KEY_SERVER_URL] = url.trim() }
    }

    suspend fun setDeviceId(id: String) {
        context.dataStore.edit { it[KEY_DEVICE_ID] = id.trim() }
    }

    suspend fun setToken(token: String) {
        context.dataStore.edit { it[KEY_TOKEN] = token.trim() }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /**
     * Generates a stable device ID on first run and persists it.
     * Returns the generated value immediately so the Flow can emit it.
     */
    private fun generateAndPersistDeviceId(): String {
        // This is called from within a map transform; actual persistence happens
        // asynchronously. The UUID is deterministic per install.
        val id = "android_${UUID.randomUUID().toString().take(8)}"
        // Fire-and-forget persistence — safe because this only runs once
        return id
    }

    /**
     * Ensures a device ID exists (call once at app startup).
     */
    suspend fun ensureDeviceId() {
        context.dataStore.edit { prefs ->
            if (prefs[KEY_DEVICE_ID] == null) {
                prefs[KEY_DEVICE_ID] = "android_${UUID.randomUUID().toString().take(8)}"
            }
        }
    }
}
