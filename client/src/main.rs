mod api;
mod bluetooth;
mod config;
mod discover;
mod errors;

use std::time::Duration;

use clap::{Parser, Subcommand};
use colored::Colorize;

use api::{GatewayClient, validate_phone};
use bluetooth::{list_bt_cards, mac_to_card_name, switch_to_a2dp, switch_to_hfp};
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

        /// Bluetooth MAC of your phone (e.g. AA:BB:CC:DD:EE:FF).
        /// When supplied, the BT card is automatically switched to HFP before
        /// the call so audio routes to your laptop speakers/mic.
        #[arg(long, value_name = "MAC")]
        bt_mac: Option<String>,
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

    /// Bluetooth audio helpers (Linux: PipeWire / PulseAudio)
    Bt {
        #[command(subcommand)]
        action: BtCmd,
    },
}

#[derive(Subcommand)]
enum BtCmd {
    /// List Bluetooth audio devices visible to PipeWire / PulseAudio
    List,

    /// Switch a paired phone to HFP call-audio profile
    ///
    /// Example:  dial bt hfp AA:BB:CC:DD:EE:FF
    Hfp {
        /// Bluetooth MAC address of the phone (AA:BB:CC:DD:EE:FF)
        mac: String,
    },

    /// Switch a phone back to A2DP stereo (music) profile
    ///
    /// Example:  dial bt a2dp AA:BB:CC:DD:EE:FF
    A2dp {
        /// Bluetooth MAC address of the phone (AA:BB:CC:DD:EE:FF)
        mac: String,
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

    /// Save a Bluetooth MAC to config so `dial call` auto-switches BT
    ///
    /// Example:  dial config set-bt-mac B8:EA:98:EF:B4:A5
    SetBtMac {
        /// Bluetooth MAC address (AA:BB:CC:DD:EE:FF)
        mac: String,
    },
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
        // ── dial call <device_id> <number> [--bt-mac MAC] ──────────────────────
        Commands::Call { device_id, number, bt_mac } => {
            if device_id.trim().is_empty() {
                return Err(DialError::EmptyDeviceId);
            }
            validate_phone(&number)?;

            // ── Resolve BT MAC: CLI flag takes precedence, then config fallback ─────
            let config = resolve_config(timeout_secs).await?;
            let effective_bt_mac = bt_mac.or_else(|| config.bt_mac.clone());

            // ── Optional: auto-switch BT to HFP before the call ──────────────
            let bt_card_name = effective_bt_mac.as_deref().map(mac_to_card_name);

            if let Some(ref card) = bt_card_name {
                use bluetooth::HfpCodec;
                print!("{} Switching Bluetooth to HFP call-audio mode… ", "♫".cyan());
                match switch_to_hfp(card) {
                    Ok(HfpCodec::PhoneGateway) => {
                        println!("{} (Audio Gateway — phone HFP active)", "done".green().bold());
                    }
                    Ok(codec) => println!("{} ({})", "done".green().bold(), codec.label()),
                    Err(e)    => {
                        eprintln!();
                        eprintln!("{} BT switch failed: {e}", "warn:".yellow());
                        eprintln!("  Continuing — audio will stay on the phone speaker.");
                    }
                }
            }

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

            // ── Remind the user how to restore audio after the call ───────────
            if let Some(ref card) = bt_card_name {
                let mac_display = effective_bt_mac.as_deref().unwrap_or("");
                println!();
                println!(
                    "  {} Audio is now routed to your laptop via BT HFP.",
                    "♫".cyan()
                );
                println!(
                    "  When the call ends, run: {}",
                    format!("dial bt a2dp {mac_display}").cyan()
                );
                let _ = card; // suppress unused-variable warning on non-Linux
            }
        }

        // ── dial devices ──────────────────────────────────────────────────────
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
                match &config.bt_mac {
                    Some(mac) if !mac.is_empty() => println!("bt_mac     = \"{}\"", mac.cyan()),
                    _ => println!("bt_mac     = {} (set to auto-switch BT on every call)", "(not set)".dimmed()),
                }
            }

            ConfigCmd::SetBtMac { mac } => {
                let mut config = Config::load()?;
                config.bt_mac = Some(mac.clone());
                config.save()?;
                println!(
                    "{} Saved bt_mac = {} to config",
                    "✓".green().bold(),
                    mac.cyan()
                );
                println!(
                    "  {} will now auto-switch BT to HFP before every call.",
                    "dial call".cyan()
                );
            }
        },

        // ── dial bt ────────────────────────────────────────────────────────────
        Commands::Bt { action } => match action {

            // dial bt list
            BtCmd::List => {
                let cards = list_bt_cards();
                if cards.is_empty() {
                    #[cfg(target_os = "linux")]
                    println!(
                        "{} No Bluetooth audio devices found.\n  \
                         Make sure your phone is paired and BT is enabled.",
                        "○".dimmed()
                    );
                    #[cfg(not(target_os = "linux"))]
                    println!(
                        "{} `dial bt list` only works on Linux (pactl required).\n  \
                         On Windows / macOS, open Sound settings to view BT devices.",
                        "!".yellow()
                    );
                } else {
                    println!("{} {} Bluetooth device(s) found\n", "●".green().bold(), cards.len());
                    for card in &cards {
                        let name = card.display_name.as_deref().unwrap_or("(unknown)");
                        let profile = match card.active_profile.as_deref() {
                            Some("headset-head-unit-msbc")  => "HFP mSBC (16 kHz)".green().to_string(),
                            Some("headset-head-unit")       => "HFP call audio".green().to_string(),
                            Some("headset-head-unit-cvsd")  => "HFP CVSD (8 kHz)".yellow().to_string(),
                            Some("audio-gateway")           => "HFP Audio Gateway".green().to_string(),
                            Some(p) if p.starts_with("a2dp") => "A2DP stereo".cyan().to_string(),
                            Some("off") | Some("")          => "off".dimmed().to_string(),
                            Some(p)                         => p.dimmed().to_string(),
                            None                            => "unknown".dimmed().to_string(),
                        };
                        println!(
                            "  {} {}  {}  ({})",
                            "─".dimmed(),
                            card.mac.cyan(),
                            name.yellow(),
                            profile,
                        );
                    }
                    println!();
                    println!(
                        "  Switch to call audio : {}",
                        "dial bt hfp <MAC>".cyan()
                    );
                    println!(
                        "  Switch back to music : {}",
                        "dial bt a2dp <MAC>".cyan()
                    );
                }
            }

            // dial bt hfp <mac>
            BtCmd::Hfp { mac } => {
                use bluetooth::HfpCodec;
                let card = mac_to_card_name(&mac);
                print!("{} Switching {} to HFP call-audio mode… ", "♫".cyan(), mac.yellow());
                match switch_to_hfp(&card) {
                    Ok(HfpCodec::PhoneGateway) => {
                        println!("{}", "done".green().bold());
                        println!("  {} Phone is in Audio Gateway mode — laptop is the HF unit.", "✓".green());
                        println!("  Audio will be bridged to your laptop as soon as a call is active.");
                        println!("  Set your default audio output to the phone in your audio settings if needed.");
                    }
                    Ok(codec) => {
                        println!("{} ({})", "done".green().bold(), codec.label());
                        println!(
                            "  {} Audio will now route to your laptop when a call is active.",
                            "✓".green()
                        );
                        println!(
                            "  To restore music audio after the call: {}",
                            format!("dial bt a2dp {mac}").cyan()
                        );
                    }
                    Err(e) => {
                        eprintln!("{}", "failed".red().bold());
                        eprintln!("{} {e}", "error:".red().bold());
                        std::process::exit(1);
                    }
                }
            }

            // dial bt a2dp <mac>
            BtCmd::A2dp { mac } => {
                let card = mac_to_card_name(&mac);
                print!("{} Switching {} back to A2DP stereo… ", "♫".cyan(), mac.yellow());
                match switch_to_a2dp(&card) {
                    Ok(()) => println!("{}", "done".green().bold()),
                    Err(e) => {
                        eprintln!("{}", "failed".red().bold());
                        eprintln!("{} {e}", "error:".red().bold());
                        std::process::exit(1);
                    }
                }
            }
        },
    }

    Ok(())
}

