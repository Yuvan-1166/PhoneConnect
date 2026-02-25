use thiserror::Error;

#[derive(Debug, Error)]
pub enum DialError {
    // ── Config ────────────────────────────────────────────────────────────────
    #[error("Config file not found at {path}.\nRun `dial config init` to create one.")]
    ConfigNotFound { path: String },

    #[error("Failed to read config file: {0}")]
    ConfigRead(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    ConfigParse(#[from] toml::de::Error),

    // ── Validation ────────────────────────────────────────────────────────────
    #[error("Invalid phone number '{0}'. Use E.164 format, e.g. +919876543210")]
    InvalidPhoneNumber(String),

    #[error("Device ID must not be empty")]
    EmptyDeviceId,

    // ── API ───────────────────────────────────────────────────────────────────
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Gateway returned {status}: {body}")]
    GatewayError { status: u16, body: String },

    #[error("Device '{device_id}' is not connected to the gateway")]
    DeviceOffline { device_id: String },

    #[error("Unauthorized — check the token in your config file")]
    Unauthorized,

    // ── Bluetooth ─────────────────────────────────────────────────────────────
    #[error("Bluetooth error: {0}")]
    Bluetooth(String),
}
