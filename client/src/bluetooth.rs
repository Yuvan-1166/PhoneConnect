/// Bluetooth HFP helpers — Linux (PipeWire / PulseAudio) focused.
///
/// ## Why the old approach produced silence
///
/// `pactl set-card-profile … headset-head-unit` only tells PipeWire that the
/// device *is capable* of HFP.  It does NOT open the Bluetooth SCO audio
/// socket.  A Bluetooth SCO socket only opens when PipeWire has an active
/// audio stream (a "running" node) connected to the HFP sink or source.
/// Until that happens the nodes stay SUSPENDED and the phone sees dead silence
/// in both directions.
///
/// ## The fix: `HfpSession`
///
/// After switching the card profile, we spawn two `pw-loopback` processes:
///
///   ① mic-loopback   — captures from `bluez_input.<MAC>` (headset mic /
///                       phone earpiece output) and plays to the laptop's
///                       default output.  This activates the SCO inbound path
///                       so you can *hear* the call.
///
///   ② speaker-loopback — captures from the laptop's default microphone and
///                         plays into `bluez_output.<MAC>` (headset speaker /
///                         phone earpiece input).  This activates the SCO
///                         outbound path so the other party can *hear you*.
///
/// Both loopbacks run for the duration of the call.  Dropping [`HfpSession`]
/// kills them cleanly and restores the card to A2DP.
///
/// ## Platform scope
/// On Windows / macOS the OS handles profile-switching and SCO automatically
/// once the device is set as the Default Communications Device.

// ── Types ─────────────────────────────────────────────────────────────────────

/// A Bluetooth audio card visible to PipeWire / PulseAudio.
#[derive(Debug, Clone)]
pub struct BtCard {
    /// Full card name, e.g. `bluez_card.AA_BB_CC_DD_EE_FF`
    pub name: String,
    /// Human-readable MAC, e.g. `AA:BB:CC:DD:EE:FF`
    pub mac: String,
    /// Friendly name from bluetoothctl (e.g. "Airdopes 163"), if available
    pub display_name: Option<String>,
    /// Currently active PulseAudio/PipeWire profile
    pub active_profile: Option<String>,
}

/// Which HFP codec / mode was activated.
#[derive(Debug, Clone, PartialEq)]
pub enum HfpCodec {
    /// mSBC — wideband 16 kHz, best quality
    MSbc,
    /// CVSD — narrowband 8 kHz, fallback
    Cvsd,
    /// Phone card in Audio-Gateway mode (the phone itself is the HFP gateway)
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

// ── HfpSession ────────────────────────────────────────────────────────────────

/// A live HFP call-audio session.
///
/// Holds the two `pw-loopback` child processes that keep the Bluetooth SCO
/// socket open.  Dropping this value kills the loopbacks and restores A2DP.
///
/// Obtain via [`activate_hfp`].
pub struct HfpSession {
    /// PulseAudio/PipeWire card name (e.g. `bluez_card.AA_5F_BE_3C_B5_01`)
    pub card_name: String,
    /// Codec that was activated
    pub codec: HfpCodec,
    #[cfg(target_os = "linux")]
    inner: Option<inner::HfpSessionInner>,
}

impl Drop for HfpSession {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if let Some(mut s) = self.inner.take() {
                s.teardown(&self.card_name);
            }
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Convert `XX:XX:XX:XX:XX:XX` (or `XX_XX_…`) to `bluez_card.XX_XX_XX_XX_XX_XX`.
pub fn mac_to_card_name(mac: &str) -> String {
    format!("bluez_card.{}", mac.replace(':', "_"))
}

/// Convert a pactl card name back to `AA:BB:CC:DD:EE:FF`.
#[allow(dead_code)]
pub fn card_name_to_mac(name: &str) -> String {
    name.trim_start_matches("bluez_card.")
        .replace('_', ":")
}

/// List all Bluetooth audio cards visible to PipeWire / PulseAudio.
pub fn list_bt_cards() -> Vec<BtCard> {
    inner::list_bt_cards()
}

/// Switch the card to HFP **and** open the Bluetooth SCO audio socket.
///
/// Returns an [`HfpSession`] that keeps both directions of call audio alive.
/// Drop it to restore A2DP once the call ends.
pub fn activate_hfp(card_name: &str) -> Result<HfpSession, String> {
    inner::activate_hfp(card_name)
}

/// Low-level profile switch only — does NOT open the SCO socket.
///
/// Use this for `dial bt hfp <MAC>` (manual inspection).
/// For live calls use [`activate_hfp`] which also opens the SCO socket.
pub fn switch_to_hfp(card_name: &str) -> Result<HfpCodec, String> {
    inner::switch_to_hfp(card_name)
}

/// Switch the given card back to the best available A2DP profile.
///
/// You normally don't need to call this — drop the [`HfpSession`] instead.
/// Kept as a stand-alone helper for `dial bt a2dp <MAC>`.
pub fn switch_to_a2dp(card_name: &str) -> Result<(), String> {
    inner::switch_to_a2dp(card_name)
}

// ── Linux implementation ──────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod inner {
    use super::{BtCard, HfpCodec, HfpSession};
    use std::process::{Child, Command};
    use std::thread;
    use std::time::{Duration, Instant};

    // ── HfpSessionInner ───────────────────────────────────────────────────────

    pub struct HfpSessionInner {
        /// pw-loopback: headset-mic → laptop-speaker  (keeps SCO RX path alive)
        mic_loopback: Option<Child>,
        /// pw-loopback: laptop-mic  → headset-speaker (keeps SCO TX path alive)
        speaker_loopback: Option<Child>,
    }

    impl HfpSessionInner {
        pub fn teardown(&mut self, card_name: &str) {
            kill_child(&mut self.mic_loopback);
            kill_child(&mut self.speaker_loopback);
            // Brief pause so PipeWire deregisters the streams before we change
            // the profile, avoiding a rare profile-change conflict.
            thread::sleep(Duration::from_millis(200));
            let _ = switch_to_a2dp(card_name);
            // Re-enable WP autoswitch so normal BT auto-selection resumes
            enable_wp_autoswitch();
        }
    }

    fn kill_child(child: &mut Option<Child>) {
        if let Some(ref mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }
        *child = None;
    }

    // ── Core: activate_hfp ───────────────────────────────────────────────────

    /// Switch to HFP and open the SCO audio socket via pw-loopback.
    ///
    /// Steps:
    ///   1. Disable WirePlumber's autoswitch-to-headset-profile policy.
    ///      Without this WP immediately reverts the profile back to A2DP
    ///      whenever no active capture stream is connected (race condition).
    ///   2. Switch card to the best HFP profile.
    ///   3. Poll for `bluez_input` / `bluez_output` nodes to materialise (≤4 s).
    ///   4. Spawn mic-loopback  (BT source → laptop sink).
    ///   5. Spawn speaker-loopback (laptop source → BT sink).
    ///   6. Poll until both nodes enter RUNNING state (≤4 s).
    ///
    /// On teardown (via Drop) the loopbacks are killed, the profile is restored
    /// to A2DP, and WP autoswitch is re-enabled.
    pub fn activate_hfp(card_name: &str) -> Result<HfpSession, String> {
        // Step 1 — disable WP autoswitch to prevent it from racing the profile
        // switch back to A2DP before our loopbacks are alive.
        disable_wp_autoswitch();

        // Step 2 — profile switch
        let codec = match switch_to_hfp(card_name) {
            Ok(c) => c,
            Err(e) => {
                // Re-enable autoswitch on failure so the system returns to normal
                enable_wp_autoswitch();
                return Err(e);
            }
        };

        // Phone gateway: the phone manages the audio path; no local SCO to open.
        if codec == HfpCodec::PhoneGateway {
            // Re-enable autoswitch — we won't be holding any capture stream
            enable_wp_autoswitch();
            return Ok(HfpSession {
                card_name: card_name.to_string(),
                codec,
                inner: None,
            });
        }

        // Derive node names from the card name
        // bluez_card.AA_5F_BE_3C_B5_01  →  AA:5F:BE:3C:B5:01
        let mac_node  = card_name.trim_start_matches("bluez_card.").replace('_', ":");
        let bt_source = format!("bluez_input.{mac_node}");
        let bt_sink   = format!("bluez_output.{mac_node}");

        // Step 2 — wait for PipeWire to expose the HFP sink + source
        wait_for(
            || has_source(&bt_source) && has_sink(&bt_sink),
            Duration::from_secs(4),
            "HFP audio nodes did not appear in time",
        )
        .map_err(|e| {
            format!(
                "{e}\n  Expected: source={bt_source} / sink={bt_sink}\n  \
                 Make sure the headset is paired, powered, and within range."
            )
        })?;

        // Step 3 — mic-loopback: headset mic → laptop speakers
        // Capturing from the BT source forces PipeWire to open the SCO RX socket.
        let mic_loopback = Command::new("pw-loopback")
            .args([
                "--name",           "phoneconnect-hfp-mic",
                "--capture",        &bt_source,
                "--capture-props",
                    "audio.channels=1 audio.position=[MONO] media.role=Phone",
                "--playback-props",
                    "media.role=Phone node.description=PhoneConnect-call-audio",
            ])
            .spawn()
            .map_err(|e| format!("Failed to start mic-loopback: {e}"))?;

        // Step 4 — speaker-loopback: laptop mic → headset speaker
        // Playing into the BT sink forces PipeWire to open the SCO TX socket.
        let speaker_loopback = Command::new("pw-loopback")
            .args([
                "--name",           "phoneconnect-hfp-speaker",
                "--playback",       &bt_sink,
                "--playback-props",
                    "audio.channels=1 audio.position=[MONO] media.role=Phone \
                     node.description=PhoneConnect-call-mic",
                "--capture-props",
                    "media.role=Phone",
            ])
            .spawn()
            .map_err(|e| format!("Failed to start speaker-loopback: {e}"))?;

        // Step 5 — wait until both loopback nodes are RUNNING (non-fatal timeout)
        let both_running = wait_for(
            || source_is_running(&bt_source) && sink_is_running(&bt_sink),
            Duration::from_secs(4),
            "",
        )
        .is_ok();

        if !both_running {
            eprintln!(
                "warn: HFP SCO stream not yet confirmed RUNNING — \
                 call may still work within 1-2 s"
            );
        }

        Ok(HfpSession {
            card_name: card_name.to_string(),
            codec,
            inner: Some(HfpSessionInner {
                mic_loopback:     Some(mic_loopback),
                speaker_loopback: Some(speaker_loopback),
            }),
        })
    }

    // ── WirePlumber policy helpers ─────────────────────────────────────────────

    /// Disable WirePlumber's autoswitch-to-headset-profile setting.
    ///
    /// Without this, WP races our profile switch: as soon as we set the card
    /// to headset-head-unit it sees no active capture stream and immediately
    /// reverts back to A2DP.  We disable the policy for the duration of the
    /// call and re-enable it in teardown.
    fn disable_wp_autoswitch() {
        let _ = Command::new("wpctl")
            .args(["settings", "bluetooth.autoswitch-to-headset-profile", "false"])
            .status();
    }

    /// Re-enable WirePlumber's autoswitch-to-headset-profile setting.
    fn enable_wp_autoswitch() {
        let _ = Command::new("wpctl")
            .args(["settings", "bluetooth.autoswitch-to-headset-profile", "true"])
            .status();
    }

    // ── Polling helpers ───────────────────────────────────────────────────────

    /// Spin-poll `condition` every 200 ms for up to `timeout`.
    fn wait_for<F>(mut condition: F, timeout: Duration, err_msg: &str) -> Result<(), String>
    where
        F: FnMut() -> bool,
    {
        let deadline = Instant::now() + timeout;
        loop {
            if condition() { return Ok(()); }
            if Instant::now() >= deadline {
                return Err(err_msg.to_string());
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn has_source(name: &str) -> bool {
        let out = run("pactl", &["list", "sources", "short"]);
        String::from_utf8_lossy(&out).contains(name)
    }

    fn has_sink(name: &str) -> bool {
        let out = run("pactl", &["list", "sinks", "short"]);
        String::from_utf8_lossy(&out).contains(name)
    }

    fn source_is_running(name: &str) -> bool {
        let out = String::from_utf8_lossy(&run("pactl", &["list", "sources", "short"])).to_string();
        out.lines()
            .filter(|l| l.contains(name))
            .any(|l| !l.contains("SUSPENDED"))
    }

    fn sink_is_running(name: &str) -> bool {
        let out = String::from_utf8_lossy(&run("pactl", &["list", "sinks", "short"])).to_string();
        out.lines()
            .filter(|l| l.contains(name))
            .any(|l| !l.contains("SUSPENDED"))
    }

    // ── Profile switching ─────────────────────────────────────────────────────

    /// Switch the card to the best available HFP profile (profile switch only).
    pub fn switch_to_hfp(card_name: &str) -> Result<HfpCodec, String> {
        let profiles = available_profiles(card_name);

        // Distinguish PipeWire vs PulseAudio naming.
        // PipeWire : headset-head-unit (mSBC) + headset-head-unit-cvsd (CVSD)
        // PulseAudio: headset-head-unit-msbc  + headset-head-unit
        let has_cvsd_explicit = profiles.iter().any(|p| p == "headset-head-unit-cvsd");
        let has_headset       = profiles.iter().any(|p| p.starts_with("headset-head-unit"));
        let has_gateway       = profiles.iter().any(|p| p == "audio-gateway");

        if has_headset {
            let (msbc, cvsd) = if has_cvsd_explicit {
                ("headset-head-unit",      "headset-head-unit-cvsd")  // PipeWire
            } else {
                ("headset-head-unit-msbc", "headset-head-unit")       // PulseAudio
            };
            if try_set(card_name, msbc) { return Ok(HfpCodec::MSbc); }
            if try_set(card_name, cvsd) { return Ok(HfpCodec::Cvsd); }
        }

        if has_gateway {
            if try_set(card_name, "audio-gateway") {
                return Ok(HfpCodec::PhoneGateway);
            }
        }

        Err(format!(
            "Could not switch {card_name} to HFP.\n\
             Make sure the device is paired, connected, and Bluetooth is on.\n\
             Available profiles: {profiles:?}"
        ))
    }

    /// Switch the card back to the best available A2DP profile.
    pub fn switch_to_a2dp(card_name: &str) -> Result<(), String> {
        let profiles = available_profiles(card_name);
        let candidates = [
            "a2dp-sink",
            "a2dp-sink-aac",
            "a2dp-sink-sbc_xq",
            "a2dp-sink-sbc",
        ];
        for p in &candidates {
            if profiles.is_empty() || profiles.iter().any(|ap| ap == *p) {
                if try_set(card_name, p) { return Ok(()); }
            }
        }
        Err(format!(
            "Could not switch {card_name} back to A2DP.\n\
             Available profiles: {profiles:?}"
        ))
    }

    // ── pactl helpers ─────────────────────────────────────────────────────────

    pub fn run(cmd: &str, args: &[&str]) -> Vec<u8> {
        Command::new(cmd)
            .args(args)
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default()
    }

    fn try_set(card_name: &str, profile: &str) -> bool {
        Command::new("pactl")
            .args(["set-card-profile", card_name, profile])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn available_profiles(card_name: &str) -> Vec<String> {
        let out  = run("pactl", &["list", "cards"]);
        let text = String::from_utf8_lossy(&out);

        let mut in_card  = false;
        let mut in_profs = false;
        let mut profiles = vec![];

        for line in text.lines() {
            let trimmed = line.trim();

            if trimmed == format!("Name: {card_name}") {
                in_card  = true;
                in_profs = false;
                profiles.clear();
                continue;
            }
            if !in_card { continue; }

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
                if let Some(name) = trimmed.split(':').next() {
                    let n = name.trim().to_string();
                    if !n.is_empty() { profiles.push(n); }
                }
            }
        }
        profiles
    }

    // ── Device listing ────────────────────────────────────────────────────────

    pub fn list_bt_cards() -> Vec<BtCard> {
        let short_out = run("pactl", &["list", "cards", "short"]);
        let mut cards: Vec<BtCard> = String::from_utf8_lossy(&short_out)
            .lines()
            .filter_map(|line| {
                let mut cols = line.splitn(3, '\t');
                let _index = cols.next()?.trim().parse::<u32>().ok()?;
                let name   = cols.next()?.trim();
                if !name.starts_with("bluez_card.") { return None; }
                let mac = name.trim_start_matches("bluez_card.").replace('_', ":");
                Some(BtCard {
                    name:           name.to_string(),
                    mac,
                    display_name:   None,
                    active_profile: None,
                })
            })
            .collect();

        if cards.is_empty() { return cards; }

        let verbose_out = run("pactl", &["list", "cards"]);
        let verbose     = String::from_utf8_lossy(&verbose_out);
        let mut current_card: Option<String> = None;

        for line in verbose.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Name:") {
                current_card = Some(rest.trim().to_string());
            }
            if let Some(rest) = trimmed.strip_prefix("Active Profile:") {
                let profile = rest.trim().to_string();
                if let Some(ref cname) = current_card {
                    if let Some(card) = cards.iter_mut().find(|c| &c.name == cname) {
                        card.active_profile = Some(profile);
                    }
                }
            }
        }

        for card in &mut cards {
            let info = run("bluetoothctl", &["info", &card.mac]);
            let text = String::from_utf8_lossy(&info);
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
}

// ── Non-Linux stub ─────────────────────────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
mod inner {
    use super::{BtCard, HfpCodec, HfpSession};

    pub fn list_bt_cards() -> Vec<BtCard> { vec![] }

    pub fn activate_hfp(_card_name: &str) -> Result<HfpSession, String> {
        Err(
            "Automatic BT HFP / SCO activation via pw-loopback is Linux-only.\n\
             On Windows: set the headset as Default Communications Device in Sound settings.\n\
             On macOS:   select the headset as input/output in System Settings → Sound."
                .to_string(),
        )
    }

    pub fn switch_to_hfp(_card_name: &str) -> Result<HfpCodec, String> {
        Err("Automatic BT profile switching is Linux-only.".to_string())
    }

    pub fn switch_to_a2dp(_card_name: &str) -> Result<(), String> {
        Err("Automatic BT profile switching is Linux-only.".to_string())
    }
}
