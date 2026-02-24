use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::errors::DialError;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct CallRequest<'a> {
    #[serde(rename = "deviceId")]
    device_id: &'a str,
    number: &'a str,
}

#[derive(Debug, Deserialize)]
struct CallResponse {
    ok: bool,
    #[serde(rename = "commandId")]
    command_id: Option<String>,
    #[serde(rename = "deviceId")]
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: Option<String>,
    reason: Option<String>,
    #[serde(rename = "deviceId")]
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceInfo {
    #[serde(rename = "deviceId")]
    pub device_id: String,
    #[serde(rename = "connectedAt")]
    pub connected_at: String,
}

#[derive(Debug, Deserialize)]
pub struct DevicesResponse {
    pub count: u32,
    pub devices: Vec<DeviceInfo>,
}

// ── Result types returned to main ─────────────────────────────────────────────

pub struct CallResult {
    pub device_id: String,
    pub command_id: String,
}

// ── API client ────────────────────────────────────────────────────────────────

pub struct GatewayClient {
    client: Client,
    base_url: String,
    token: String,
}

impl GatewayClient {
    pub fn new(config: &Config) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url: config.server_url.trim_end_matches('/').to_string(),
            token: config.token.clone(),
        }
    }

    // ── POST /call ────────────────────────────────────────────────────────────

    /// Send a CALL command to `device_id` for the given `number`.
    pub async fn call(&self, device_id: &str, number: &str) -> Result<CallResult, DialError> {
        let url = format!("{}/call", self.base_url);

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&CallRequest { device_id, number })
            .send()
            .await?;

        let status = response.status();

        match status.as_u16() {
            200 => {
                let body: CallResponse = response.json().await?;
                Ok(CallResult {
                    device_id: body.device_id.unwrap_or_else(|| device_id.to_string()),
                    command_id: body.command_id.unwrap_or_default(),
                })
            }
            401 => Err(DialError::Unauthorized),
            404 => Err(DialError::DeviceOffline {
                device_id: device_id.to_string(),
            }),
            status_code => {
                let body: ErrorResponse = response.json().await.unwrap_or(ErrorResponse {
                    error: Some("Unknown error".to_string()),
                    reason: None,
                    device_id: None,
                });
                let msg = body.reason
                    .or(body.error)
                    .unwrap_or_else(|| "Unknown error".to_string());
                Err(DialError::GatewayError {
                    status: status_code,
                    body: msg,
                })
            }
        }
    }

    // ── GET /devices ──────────────────────────────────────────────────────────

    /// List all devices currently connected to the gateway.
    pub async fn devices(&self) -> Result<DevicesResponse, DialError> {
        let url = format!("{}/devices", self.base_url);
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        match response.status().as_u16() {
            200 => Ok(response.json::<DevicesResponse>().await?),
            401 => Err(DialError::Unauthorized),
            code => Err(DialError::GatewayError {
                status: code,
                body: response.text().await.unwrap_or_default(),
            }),
        }
    }

    // ── GET /health ───────────────────────────────────────────────────────────

    /// Check if the gateway is reachable.
    pub async fn health(&self) -> Result<serde_json::Value, DialError> {
        let url = format!("{}/health", self.base_url);
        let response = self.client.get(&url).send().await?;
        Ok(response.json::<serde_json::Value>().await?)
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Basic E.164 validation: optional +, 7–15 digits.
pub fn validate_phone(number: &str) -> Result<(), DialError> {
    let digits_only = number.strip_prefix('+').unwrap_or(number);
    let all_digits = digits_only.chars().all(|c| c.is_ascii_digit());
    let len_ok = digits_only.len() >= 7 && digits_only.len() <= 15;

    if !all_digits || !len_ok {
        return Err(DialError::InvalidPhoneNumber(number.to_string()));
    }
    Ok(())
}
