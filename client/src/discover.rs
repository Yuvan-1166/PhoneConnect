use std::net::IpAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

/// The mDNS service type published by the gateway's bonjour-service.
const SERVICE_TYPE: &str = "_phoneconnect._tcp.local.";

// ── Result type ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DiscoveredGateway {
    /// Ready-to-use HTTP base URL: "http://10.0.0.5:3000"
    pub url:  String,
    pub host: String,
    pub port: u16,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Scan the local network for a PhoneConnect gateway via mDNS/DNS-SD.
///
/// Algorithm:
///   1. Start a `ServiceDaemon` (pure-Rust mDNS stack, no system deps).
///   2. Browse `_phoneconnect._tcp.local.` — the same service type the gateway
///      advertises using `bonjour-service`.
///   3. Return the **first** resolved service that has a usable IP address.
///   4. Stop browsing and return.
///
/// Runs on a blocking thread via `spawn_blocking` so it doesn't stall the
/// async executor during the channel‐based wait loop.
///
/// Returns `None` if nothing is found within `timeout`.
pub async fn discover_gateway(timeout: Duration) -> Option<DiscoveredGateway> {
    tokio::task::spawn_blocking(move || discover_blocking(timeout))
        .await
        .ok()
        .flatten()
}

// ── Blocking implementation ───────────────────────────────────────────────────

fn discover_blocking(timeout: Duration) -> Option<DiscoveredGateway> {
    let mdns = ServiceDaemon::new()
        .inspect_err(|e| eprintln!("mDNS daemon error: {e}"))
        .ok()?;

    let receiver = mdns
        .browse(SERVICE_TYPE)
        .inspect_err(|e| eprintln!("mDNS browse error: {e}"))
        .ok()?;

    let deadline = Instant::now() + timeout;

    let result = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break None;
        }

        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                // Prefer a non-loopback IPv4 address; fall back to any address
                let addr: IpAddr = info
                    .get_addresses()
                    .iter()
                    .find(|a| a.is_ipv4() && !a.is_loopback() && !is_link_local(a))
                    .or_else(|| info.get_addresses().iter().find(|a| !a.is_loopback()))
                    .copied()
                    .or_else(|| info.get_addresses().iter().copied().next())?;

                let host = addr.to_string();
                let port = info.get_port();
                let url  = format!("http://{}:{}", host, port);

                break Some(DiscoveredGateway { url, host, port });
            }
            Ok(_) => continue, // SearchStarted, ServiceFound, ServiceRemoved — skip
            Err(_) => break None, // channel closed or timed out
        }
    };

    // Clean up regardless of outcome
    let _ = mdns.stop_browse(SERVICE_TYPE);
    let _ = mdns.shutdown();

    result
}

/// Link-local addresses (169.254.x.x / fe80::) are not routable — skip them.
fn is_link_local(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4.octets()[0] == 169 && v4.octets()[1] == 254,
        IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}
