mod api;
mod config;
mod discover;
mod errors;

use std::time::Duration;

use clap::{Parser, Subcommand};
use colored::Colorize;

use api::{GatewayClient, validate_phone};
use config::Config;
use discover::discover_gateway;
use errors::DialError;

// ── CLI definition ─────────────────────────────────────────────────────────────

/// PhoneConnect CLI — trigger phone calls through your Android device
#[derive(Parser)]
#[command(name = "dial", version, about, long_about = None)]
struct Cli {
    /// Override discovery timeout in seconds (default: 5)
    #[arg(long, global = true, default_value = "5")]
    timeout: u64,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initiate a phone call via a connected Android device
    Call {
        /// Device ID shown in the PhoneConnect app (e.g. android_fd9de1fb)
        device_id: String,

        /// Phone number in E.164 format (e.g. +919876543210)
        number: String,
    },

    /// List devices currently connected to the gateway
    Devices,

    /// Check gateway health
    Status,

    /// Scan the LAN for a PhoneConnect gateway and save its URL to config
    Discover,

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Create a default config file at ~/.config/phoneconnect/config.toml
    Init,

    /// Print the path to the config file
    Path,

    /// Show current config values
    Show,
}

// ── Entry point ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match run(cli).await {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    }
}

// ── Config resolution with auto-discovery ─────────────────────────────────────

/// Load config and, if the URL is still the placeholder, auto-discover the
/// gateway via mDNS — same as typing `dial discover` but transparent.
///
/// If discovery finds a gateway the new URL is **persisted** to the config file
/// so the next invocation is instant (no re-scan unless the IP changes again).
async fn resolve_config(timeout_secs: u64) -> Result<Config, DialError> {
    // Load or create a default config
    let mut cfg = match Config::load() {
        Ok(c) => c,
        Err(DialError::ConfigNotFound { .. }) => {
            // First run: write defaults and proceed to discovery
            Config::write_default()?;
            Config::load()?
        }
        Err(e) => return Err(e),
    };

    if cfg.is_placeholder() {
        println!(
            "{} No gateway URL configured — scanning LAN ({timeout_secs}s)…",
            "◎".cyan()
        );

        let timeout = Duration::from_secs(timeout_secs);
        match discover_gateway(timeout).await {
            Some(found) => {
                println!(
                    "{} Gateway found at {}:{} — saving to config",
                    "✓".green().bold(),
                    found.host.cyan(),
                    found.port.to_string().cyan(),
                );
                cfg.server_url = found.url;
                // Persist so next run skips the scan
                if let Err(e) = cfg.save() {
                    eprintln!("{} Could not save config: {e}", "warn:".yellow());
                }
            }
            None => {
                eprintln!(
                    "{} No gateway found on LAN within {timeout_secs}s.",
                    "!".yellow()
                );
                eprintln!(
                    "  Start the gateway on your laptop, or run {} to set the URL manually.",
                    "dial config init".cyan()
                );
                return Err(DialError::GatewayError {
                    status: 0,
                    body: "Gateway not found via mDNS. Make sure the server is running on the same network.".into(),
                });
            }
        }
    }

    cfg.validate()?;
    Ok(cfg)
}

// ── Command handlers ───────────────────────────────────────────────────────────

async fn run(cli: Cli) -> Result<(), DialError> {
    let timeout_secs = cli.timeout;

    match cli.command {
        // ── dial call <device_id> <number> ─────────────────────────────────────
        Commands::Call { device_id, number } => {
            if device_id.trim().is_empty() {
                return Err(DialError::EmptyDeviceId);
            }
            validate_phone(&number)?;

            let config = resolve_config(timeout_secs).await?;
            let client = GatewayClient::new(&config);

            println!(
                "{} Dispatching call to {} → {}",
                "→".cyan().bold(),
                device_id.yellow(),
                number.yellow()
            );

            let result = client.call(&device_id, &number).await?;

            println!("{} Call command sent!", "✓".green().bold());
            println!("  Device : {}", result.device_id.cyan());
            println!("  Command: {}", result.command_id.dimmed());
        }

        // ── dial devices ───────────────────────────────────────────────────────
        Commands::Devices => {
            let config = resolve_config(timeout_secs).await?;
            let client = GatewayClient::new(&config);
            let resp   = client.devices().await?;

            if resp.devices.is_empty() {
                println!("{} No devices currently connected.", "○".dimmed());
            } else {
                println!("{} {} device(s) connected\n", "●".green().bold(), resp.count);
                for dev in &resp.devices {
                    println!(
                        "  {} {}  (connected since {})",
                        "─".dimmed(),
                        dev.device_id.cyan(),
                        dev.connected_at.dimmed()
                    );
                }
            }
        }

        // ── dial status ────────────────────────────────────────────────────────
        Commands::Status => {
            let config = resolve_config(timeout_secs).await?;
            let client = GatewayClient::new(&config);
            let health = client.health().await?;

            println!("{} Gateway is reachable", "✓".green().bold());
            println!("  URL:               {}", config.server_url.cyan());
            if let Some(uptime) = health.get("uptime").and_then(|v| v.as_f64()) {
                println!("  Uptime:            {:.0}s", uptime);
            }
            if let Some(count) = health.get("connectedDevices").and_then(|v| v.as_u64()) {
                println!("  Connected devices: {}", count);
            }
        }

        // ── dial discover ──────────────────────────────────────────────────────
        Commands::Discover => {
            println!(
                "{} Scanning for PhoneConnect gateway ({timeout_secs}s)…",
                "◎".cyan()
            );

            let timeout = Duration::from_secs(timeout_secs);
            match discover_gateway(timeout).await {
                Some(found) => {
                    println!(
                        "{} Gateway found!\n  Host: {}\n  Port: {}\n  URL:  {}",
                        "✓".green().bold(),
                        found.host.cyan(),
                        found.port.to_string().cyan(),
                        found.url.cyan(),
                    );

                    // Save to config
                    match Config::load() {
                        Ok(mut cfg) => {
                            cfg.server_url = found.url;
                            cfg.save()?;
                            println!(
                                "{} Saved to {}",
                                "↳".dimmed(),
                                Config::path().display().to_string().dimmed()
                            );
                        }
                        Err(_) => {
                            // No config file yet — create one
                            let path = Config::write_default()?;
                            let mut cfg = Config::load()?;
                            cfg.server_url = found.url;
                            cfg.save()?;
                            println!("{} Config created at {}", "↳".dimmed(), path.display().to_string().dimmed());
                        }
                    }
                }
                None => {
                    eprintln!(
                        "{} No gateway found within {timeout_secs}s. Is the server running?",
                        "✗".red().bold()
                    );
                    std::process::exit(1);
                }
            }
        }

        // ── dial config ────────────────────────────────────────────────────────
        Commands::Config { action } => match action {
            ConfigCmd::Init => {
                let path = Config::write_default()?;
                println!(
                    "{} Created config at {}",
                    "✓".green().bold(),
                    path.display().to_string().cyan()
                );
                println!("  server_url is set to the placeholder — run {} to auto-detect the gateway.", "dial discover".cyan());
            }
            ConfigCmd::Path => {
                println!("{}", Config::path().display());
            }
            ConfigCmd::Show => {
                let config = Config::load()?;
                println!("server_url = \"{}\"", config.server_url.cyan());
                println!("token      = \"{}\"", "***".dimmed());
            }
        },
    }

    Ok(())
}

