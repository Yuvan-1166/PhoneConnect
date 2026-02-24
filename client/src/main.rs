mod api;
mod config;
mod errors;

use clap::{Parser, Subcommand};
use colored::Colorize;

use api::{GatewayClient, validate_phone};
use config::Config;
use errors::DialError;

// ── CLI definition ─────────────────────────────────────────────────────────────

/// PhoneConnect CLI — trigger phone calls through your Android device
#[derive(Parser)]
#[command(name = "dial", version, about, long_about = None)]
struct Cli {
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

async fn run(cli: Cli) -> Result<(), DialError> {
    match cli.command {
        // ── dial call <device_id> <number> ─────────────────────────────────────
        Commands::Call { device_id, number } => {
            if device_id.trim().is_empty() {
                return Err(DialError::EmptyDeviceId);
            }
            validate_phone(&number)?;

            let config = Config::load()?;
            config.validate()?;
            let client = GatewayClient::new(&config);

            println!(
                "{} Dispatching call to {} → {}",
                "→".cyan().bold(),
                device_id.yellow(),
                number.yellow()
            );

            let result = client.call(&device_id, &number).await?;

            println!(
                "{} Call command sent!",
                "✓".green().bold()
            );
            println!("  Device : {}", result.device_id.cyan());
            println!("  Command: {}", result.command_id.dimmed());
        }

        // ── dial devices ───────────────────────────────────────────────────────
        Commands::Devices => {
            let config = Config::load()?;
            let client = GatewayClient::new(&config);
            let resp = client.devices().await?;

            if resp.devices.is_empty() {
                println!("{} No devices currently connected.", "○".dimmed());
            } else {
                println!(
                    "{} {} device(s) connected\n",
                    "●".green().bold(),
                    resp.count
                );
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
            let config = Config::load()?;
            let client = GatewayClient::new(&config);
            let health = client.health().await?;

            println!("{} Gateway is reachable", "✓".green().bold());
            println!("  URL:              {}", config.server_url.cyan());
            if let Some(uptime) = health.get("uptime").and_then(|v| v.as_f64()) {
                println!("  Uptime:           {:.0}s", uptime);
            }
            if let Some(count) = health.get("connectedDevices").and_then(|v| v.as_u64()) {
                println!("  Connected devices:{}", count);
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
                println!("  Edit it to set your server URL and token.");
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

