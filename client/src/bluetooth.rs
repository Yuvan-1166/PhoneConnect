/// Bluetooth HFP helpers — Linux (PipeWire / PulseAudio) focused.
///
/// On Linux the phone appears as a PulseAudio/PipeWire card whose profile can
/// be flipped between A2DP (stereo music) and HFP (call audio).
/// All calls route through `pactl`, which is available on every modern distro
/// that runs PipeWire or PulseAudio (Ubuntu 20.04+, Fedora 34+, Arch, etc.).
///
/// On Windows and macOS the OS manages profile switching automatically once the
/// phone is set as the default communications device; this module emits a
/// helpful message in those cases rather than silently doing nothing.

// ── Types ─────────────────────────────────────────────────────────────────────

/// A Bluetooth audio card visible to PipeWire / PulseAudio.
#[derive(Debug, Clone)]
pub struct BtCard {
    /// Full card name, e.g. `bluez_card.AA_BB_CC_DD_EE_FF`
    pub name: String,
    /// Human-readable MAC, e.g. `AA:BB:CC:DD:EE:FF`
    pub mac: String,
    /// Friendly name from bluetoothctl (e.g. "Redmi 12 5G"), if available
    pub display_name: Option<String>,
    /// Currently active PulseAudio/PipeWire profile (e.g. "headset-head-unit-msbc")
    pub active_profile: Option<String>,
}

/// Which HFP codec / mode was activated.
#[derive(Debug, Clone)]
pub enum HfpCodec {
    /// mSBC — wideband 16 kHz, best quality (headset/earbuds)
    MSbc,
    /// CVSD — narrowband 8 kHz, fallback (headset/earbuds)
    Cvsd,
    /// Phone card in Audio-Gateway mode — the phone IS the HFP gateway.
    /// Audio will route between laptop ↔ phone automatically during an active call.
    PhoneGateway,
}

impl HfpCodec {
    pub fn label(&self) -> &'static str {
        match self {
            HfpCodec::MSbc         => "mSBC (16 kHz wideband)",
            HfpCodec::Cvsd         => "CVSD (8 kHz narrowband)",
            HfpCodec::PhoneGateway => "Audio Gateway (phone HFP mode)",
        }
    }
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// Convert a MAC address in either `XX:XX:XX:XX:XX:XX` or `XX_XX_XX_XX_XX_XX`
/// format to the PulseAudio card name (`bluez_card.XX_XX_XX_XX_XX_XX`).
pub fn mac_to_card_name(mac: &str) -> String {
    format!("bluez_card.{}", mac.replace(':', "_"))
}

/// Convert a pactl card name back to a readable MAC (`AA:BB:CC:DD:EE:FF`).
#[allow(dead_code)]
pub fn card_name_to_mac(name: &str) -> String {
    name.trim_start_matches("bluez_card.")
        .replace('_', ":")
}

// ── Platform-specific implementations ────────────────────────────────────────

#[cfg(target_os = "linux")]
mod inner {
    use super::{BtCard, HfpCodec};
    use std::process::Command;

    /// List all Bluetooth audio cards currently visible to PipeWire / PulseAudio.
    pub fn list_bt_cards() -> Vec<BtCard> {
        // ── Step 1: get card list from pactl (short format) ───────────────────
        let short_out = run("pactl", &["list", "cards", "short"]);
        let mut cards: Vec<BtCard> = String::from_utf8_lossy(&short_out)
            .lines()
            .filter_map(|line| {
                // Format:  <index>\t<name>\t<module>
                let mut cols = line.splitn(3, '\t');
                let _index = cols.next()?.trim().parse::<u32>().ok()?;
                let name   = cols.next()?.trim();
                if !name.starts_with("bluez_card.") {
                    return None;
                }
                let mac = name.trim_start_matches("bluez_card.").replace('_', ":");
                Some(BtCard {
                    name:           name.to_string(),
                    mac,
                    display_name:   None,
                    active_profile: None,
                })
            })
            .collect();

        if cards.is_empty() {
            return cards;
        }

        // ── Step 2: enrich with active profile from verbose pactl output ──────
        let verbose_out = run("pactl", &["list", "cards"]);
        let verbose     = String::from_utf8_lossy(&verbose_out);
        let mut current_card: Option<String> = None;

        for line in verbose.lines() {
            let trimmed = line.trim();
            // Detect card block: "Name: bluez_card.XX_XX_XX_XX_XX_XX"
            if let Some(rest) = trimmed.strip_prefix("Name:") {
                current_card = Some(rest.trim().to_string());
            }
            // Detect active profile line within a card block
            if let Some(rest) = trimmed.strip_prefix("Active Profile:") {
                let profile = rest.trim().to_string();
                if let Some(ref cname) = current_card {
                    if let Some(card) = cards.iter_mut().find(|c| &c.name == cname) {
                        card.active_profile = Some(profile);
                    }
                }
            }
        }

        // ── Step 3: enrich with friendly device name from bluetoothctl ────────
        for card in &mut cards {
            let info = run("bluetoothctl", &["info", &card.mac]);
            let text = String::from_utf8_lossy(&info);
            // Look for "Alias:" or "Name:" line — Alias is the user-visible label
            for line in text.lines() {
                let t = line.trim();
                let name_str = t
                    .strip_prefix("Alias:")
                    .or_else(|| t.strip_prefix("Name:"))
                    .map(str::trim);
                if let Some(n) = name_str {
                    if !n.is_empty() {
                        card.display_name = Some(n.to_string());
                        break;
                    }
                }
            }
        }

        cards
    }

    /// Run a command and return its stdout bytes; returns empty vec on failure.
    fn run(cmd: &str, args: &[&str]) -> Vec<u8> {
        Command::new(cmd)
            .args(args)
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default()
    }

    /// Set a card profile via pactl; returns true on success.
    fn try_set(card_name: &str, profile: &str) -> bool {
        Command::new("pactl")
            .args(["set-card-profile", card_name, profile])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Return available profile names for a card by parsing `pactl list cards`.
    fn available_profiles(card_name: &str) -> Vec<String> {
        let out  = run("pactl", &["list", "cards"]);
        let text = String::from_utf8_lossy(&out);

        let mut in_card  = false;
        let mut in_profs = false;
        let mut profiles = vec![];

        for line in text.lines() {
            let trimmed = line.trim();

            // Detect the card we care about
            if trimmed == format!("Name: {card_name}") {
                in_card  = true;
                in_profs = false;
                profiles.clear();
                continue;
            }
            if !in_card { continue; }

            // Stop when we hit the next card block
            if trimmed.starts_with("Name: bluez_card.")
                && trimmed != format!("Name: {card_name}")
            {
                break;
            }

            if trimmed == "Profiles:" {
                in_profs = true;
                continue;
            }

            if in_profs {
                if trimmed.starts_with("Active Profile:") { break; }
                // Profile lines: "profile-name: Description…"
                if let Some(name) = trimmed.split(':').next() {
                    let n = name.trim().to_string();
                    if !n.is_empty() { profiles.push(n); }
                }
            }
        }
        profiles
    }

    /// Switch the given card to the best available HFP profile.
    ///
    /// Profile priority (handles both PipeWire and PulseAudio naming):
    ///   PipeWire: headset-head-unit (mSBC) → headset-head-unit-cvsd (CVSD)
    ///   PulseAudio: headset-head-unit-msbc → headset-head-unit
    ///   Phones: audio-gateway (IS the HFP gateway — laptop acts as HF unit)
    pub fn switch_to_hfp(card_name: &str) -> Result<HfpCodec, String> {
        let profiles = available_profiles(card_name);

        let has_cvsd_explicit = profiles.iter().any(|p| p == "headset-head-unit-cvsd");
        let has_headset       = profiles.iter().any(|p| p.starts_with("headset-head-unit"));
        let has_gateway       = profiles.iter().any(|p| p == "audio-gateway");

        if has_headset {
            // PipeWire uses distinct names: headset-head-unit = mSBC, headset-head-unit-cvsd = CVSD.
            // PulseAudio uses: headset-head-unit-msbc = mSBC, headset-head-unit = CVSD.
            let (msbc, cvsd) = if has_cvsd_explicit {
                ("headset-head-unit",      "headset-head-unit-cvsd")   // PipeWire
            } else {
                ("headset-head-unit-msbc", "headset-head-unit")        // PulseAudio
            };

            if try_set(card_name, msbc) { return Ok(HfpCodec::MSbc); }
            if try_set(card_name, cvsd) { return Ok(HfpCodec::Cvsd); }
        }

        if has_gateway {
            // Phone card — audio-gateway IS the HFP mode.
            // The laptop acts as the HF (hands-free) unit.
            if try_set(card_name, "audio-gateway") {
                return Ok(HfpCodec::PhoneGateway);
            }
        }

        Err(format!(
            "Could not switch {card_name} to HFP.\n\
             Make sure the phone is paired, connected, and Bluetooth is on.\n\
             Available profiles: {profiles:?}"
        ))
    }

    /// Switch the given card back to the best available A2DP profile.
    pub fn switch_to_a2dp(card_name: &str) -> Result<(), String> {
        let profiles = available_profiles(card_name);

        // Prefer generic name (PipeWire picks best codec) then explicit codecs.
        let candidates = ["a2dp-sink", "a2dp-sink-aac", "a2dp-sink-sbc_xq", "a2dp-sink-sbc"];

        for p in &candidates {
            if profiles.is_empty() || profiles.iter().any(|ap| ap == *p) {
                if try_set(card_name, p) {
                    return Ok(());
                }
            }
        }

        Err(format!(
            "Could not switch {card_name} back to A2DP.\n\
             Available profiles: {profiles:?}"
        ))
    }
}

// ── Non-Linux stub ────────────────────────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
mod inner {
    use super::{BtCard, HfpCodec};

    pub fn list_bt_cards() -> Vec<BtCard> {
        vec![]
    }

    pub fn switch_to_hfp(_card_name: &str) -> Result<HfpCodec, String> {
        // Windows / macOS switch HFP automatically once the device is set as
        // the default communications device — no manual command needed.
        Err(
            "Automatic BT profile switching via `pactl` is Linux-only.\n\
             On Windows: set the phone as your Default Communications Device in Sound settings.\n\
             On macOS:   select the phone as your input/output in System Settings → Sound."
                .to_string(),
        )
    }

    pub fn switch_to_a2dp(_card_name: &str) -> Result<(), String> {
        Err(
            "Automatic BT profile switching is Linux-only.\n\
             Switch back manually in your OS Sound settings."
                .to_string(),
        )
    }
}

// ── Public surface (flatten inner) ───────────────────────────────────────────

/// List all Bluetooth audio cards currently visible to PipeWire / PulseAudio.
pub fn list_bt_cards() -> Vec<BtCard> {
    inner::list_bt_cards()
}

/// Switch the given card to HFP.
/// Tries mSBC first, falls back to CVSD.
pub fn switch_to_hfp(card_name: &str) -> Result<HfpCodec, String> {
    inner::switch_to_hfp(card_name)
}

/// Switch the given card back to A2DP (stereo music) profile.
pub fn switch_to_a2dp(card_name: &str) -> Result<(), String> {
    inner::switch_to_a2dp(card_name)
}
