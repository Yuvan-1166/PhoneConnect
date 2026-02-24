use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::errors::DialError;

/// Contents of `~/.config/phoneconnect/config.toml`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// Gateway HTTP base URL, e.g. "http://10.61.214.187:3000"
    pub server_url: String,

    /// Bearer token that matches GATEWAY_TOKENS on the server
    pub token: String,
}

/// The factory-default URL written by `config init`.
/// If the config still has this value, auto-discovery is triggered.
pub const PLACEHOLDER_URL: &str = "http://10.61.214.187:3000";

impl Config {
    // ── Paths ─────────────────────────────────────────────────────────────────

    /// Returns the path to the config file.
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("phoneconnect")
            .join("config.toml")
    }

    // ── Load ──────────────────────────────────────────────────────────────────

    /// Load and parse the config file.
    /// Returns [DialError::ConfigNotFound] with the expected path if missing.
    pub fn load() -> Result<Self, DialError> {
        let path = Self::path();

        if !path.exists() {
            return Err(DialError::ConfigNotFound {
                path: path.display().to_string(),
            });
        }

        let raw = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&raw)?;
        Ok(config)
    }

    // ── Init / Save ────────────────────────────────────────────────────────────

    /// Write the default config file to the standard path.
    /// Creates parent directories if they don't exist.
    pub fn write_default() -> Result<PathBuf, DialError> {
        let path = Self::path();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let default = Config {
            server_url: PLACEHOLDER_URL.to_string(),
            token: "change-me-secret".to_string(),
        };

        let toml_str = toml::to_string_pretty(&default)
            .expect("default config must serialise");

        fs::write(&path, toml_str)?;
        Ok(path)
    }

    /// Persist the current state back to the config file.
    /// Creates parent directories if needed.
    pub fn save(&self) -> Result<(), DialError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(self)
            .expect("config must serialise");
        fs::write(&path, toml_str)?;
        Ok(())
    }

    /// Returns `true` if the URL is the unconfigured placeholder or blank.
    /// This triggers automatic mDNS discovery instead of connecting directly.
    pub fn is_placeholder(&self) -> bool {
        self.server_url.trim().is_empty() || self.server_url == PLACEHOLDER_URL
    }

    /// Validate that required fields are non-empty.
    pub fn validate(&self) -> Result<(), DialError> {
        if self.token.trim().is_empty() {
            return Err(DialError::Unauthorized);
        }
        Ok(())
    }
}
