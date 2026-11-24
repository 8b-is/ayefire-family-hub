#![deny(clippy::float_arithmetic)]
//! AyeFire Live Transparency Radar, Behavioral Micro-Beacon Detector & SSE Notification Console.
//!
//! Zero floating-point arithmetic (`#![deny(clippy::float_arithmetic)]`).
//! Zero cookie / credential / payload archival — analyzes strictly structural & timing signals:
//!   1. **Mouse-Move / Scroll Micro-Beacon Bursts**: `>= 4` small `POST`/`PUT` or telemetry-path
//!      packets within `10 seconds` on the same host (exact reduced `p/q` cadence!).
//!   2. **Invisible Tracking Pixels & 204 Beacons**: `<= 128 B` GIF/PNG tracking pixels (e.g.,
//!      `35 B image/gif`) or `HTTP 204` (`0 B`) behavioral pings (`/gen204`, `/collect`).
//!   3. **Identity Brokers & Session-Replay SDKs**: LiveRamp (`rlcdn.com`), FullStory, Hotjar,
//!      Clarity, Mouseflow, Datadog RUM, Reddit `w3-reporting`/`pi`/`alb`, etc.
//!   4. **Silent Cross-Site Third-Party Fanout**: Tracks `Referer` → 3rd-party host graph in
//!      bounded RAM so you see *which* page secretly contacted *which* trackers.
//!   5. **Unclicked Background Uploads**: Unexpected `POST >= 8 KiB` to telemetry/replay paths.
//!   6. **Instant 1-Click Countermeasures**:
//!      - **Silence Beacon Path** (`host + path_prefix`, e.g., `www.reddit.com/svc/shreddit/events`
//!        → local `204 No Content` so the site keeps working while mouse beacons never leave home!)
//!      - **Block Host** (blocks immediately across BOTH `:8792` Proxy AND `:5354` Sovereign DNS!)

use crate::phoenix_gateway::format_reduced_pq;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};

/// Sliding burst window for detecting mouse-move / scroll micro-beacons (10,000 ms).
const BURST_WINDOW_MS: u64 = 10_000;
/// Minimum small telemetry/POST packets in `BURST_WINDOW_MS` to raise a burst alert.
const BURST_MIN_EVENTS: usize = 4;
/// Cooldown per `(host, alert_kind)` to prevent notification fatigue (30 seconds).
const ALERT_COOLDOWN_SECS: u64 = 30;
/// Bounded host profile table (C64 RAM discipline).
const MAX_TRACKED_HOSTS: usize = 256;
/// Bounded alert history ring.
const MAX_ALERT_HISTORY: usize = 64;

/// Known session-replay, behavioral tracking, ad-tech, and cross-site identity broker suffixes.
const IDENTITY_OR_REPLAY_HOSTS: &[(&str, &str)] = &[
    ("rlcdn.com", "LiveRamp cross-site identity resolution broker"),
    ("fullstory.com", "FullStory DOM & mouse session-replay recorder"),
    ("hotjar.com", "Hotjar mouse heatmap & session-replay tracker"),
    ("clarity.ms", "Microsoft Clarity click/scroll/session recorder"),
    ("mouseflow.com", "Mouseflow cursor & keystroke session recorder"),
    ("logrocket.com", "LogRocket session replay telemetry"),
    ("segment.io", "Segment behavioral event pipeline"),
    ("segment.com", "Segment behavioral event pipeline"),
    ("amplitude.com", "Amplitude user behavior analytics"),
    ("mixpanel.com", "Mixpanel event & funnel tracking"),
    ("posthog.com", "PostHog product analytics / session replay"),
    ("datadoghq.com", "Datadog RUM telemetry & session replay"),
    ("browser-intake-datadoghq.com", "Datadog RUM telemetry beacon"),
    ("sentry.io", "Sentry telemetry & optional session replay"),
    ("w3-reporting.reddit.com", "Reddit background browser telemetry reporting"),
    ("pi.reddit.com", "Reddit behavioral event collector"),
    ("alb.reddit.com", "Reddit ad/attribution tracking pixel"),
    ("doubleclick.net", "Google DoubleClick ad-tracking network"),
    ("google-analytics.com", "Google Analytics behavioral tracker"),
    ("scorecardresearch.com", "Comscore cross-site audience measurement"),
    ("criteo.com", "Criteo cross-site ad retargeting"),
    ("demdex.net", "Adobe Audience Manager identity broker"),
    ("bluekai.com", "Oracle BlueKai data broker"),
    ("adsrvr.org", "The Trade Desk cross-site ad tracker"),
    ("quantserve.com", "Quantcast audience profiler"),
    ("taboola.com", "Taboola behavioral recommendation tracker"),
    ("outbrain.com", "Outbrain behavioral ad tracker"),
    ("trace.svc.ui.com", "Ubiquiti UniFi device phone-home telemetry"),
    ("ping.ui.com", "Ubiquiti UniFi connectivity beacon"),
];

/// Path substrings characteristic of behavioral beacons, mouse-move collectors, and telemetry.
const BEACON_PATH_HINTS: &[&str] = &[
    "/events",
    "/event",
    "/collect",
    "/beacon",
    "/reports",
    "/report",
    "/telemetry",
    "/metrics",
    "/gen204",
    "/gen_204",
    "/generate_204",
    "/pixel",
    "/track",
    "/replay",
    "/rrweb",
    "/rum",
    "/mouse",
    "/v4/p",
    "/skatepark",
    "/tr/",
    "/j/collect",
    "/g/collect",
    "/log?",
    "/ingest",
];

#[derive(Clone, Debug)]
pub struct TrafficObservation {
    pub host: String,
    pub referer_host: Option<String>,
    pub method: String,
    pub path_only: String,
    pub status: u16,
    pub req_bytes: u64,
    pub resp_bytes: u64,
    pub content_type: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TransparencyAlert {
    pub id: u64,
    pub severity: String, // "high" | "medium" | "info"
    pub kind: String,
    pub host: String,
    pub path_sample: String,
    pub referer_host: Option<String>,
    pub title: String,
    pub detail: String,
    pub rate_pq: String,
    pub trust_of_20: u8,
    pub suggested_action: String, // "silence_path" | "block_host" | "monitor"
    pub unix_secs: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SiteTransparencyView {
    pub host: String,
    pub trust_of_20: u8,
    pub trust_pq: String,
    pub verdict: String,
    pub total_requests: u64,
    pub post_beacons: u64,
    pub pixel_or_204_beacons: u64,
    pub burst_events_10s: usize,
    pub burst_rate_pq: String,
    pub silenced_count: u64,
    pub third_party_hosts: Vec<String>,
    pub called_by_sites: Vec<String>,
    pub sample_beacon_paths: Vec<String>,
    pub known_tracker_note: Option<String>,
    pub is_blocked: bool,
    pub last_seen_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PersistedRadarRules {
    pub blocked_hosts: BTreeSet<String>,
    /// Key format: `host|path_prefix` (e.g. `www.reddit.com|/svc/shreddit/events`)
    pub silenced_paths: BTreeSet<String>,
    pub auto_silence_beacons: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TransparencyRadarStatus {
    pub auto_silence_beacons: bool,
    pub sites_observed: usize,
    pub total_observations: u64,
    pub total_alerts: u64,
    pub micro_beacons_detected: u64,
    pub pixels_detected: u64,
    pub beacons_silenced: u64,
    pub hosts_blocked_hits: u64,
    pub blocked_hosts: Vec<String>,
    pub silenced_paths: Vec<String>,
    pub alerts: Vec<TransparencyAlert>,
    pub sites: Vec<SiteTransparencyView>,
}

struct HostBehavioralProfile {
    total_requests: u64,
    post_beacons: u64,
    pixel_or_204_beacons: u64,
    large_bg_uploads: u64,
    silenced_count: u64,
    recent_beacon_ms: VecDeque<u64>,
    third_party_hosts: BTreeSet<String>,
    called_by_sites: BTreeSet<String>,
    sample_beacon_paths: VecDeque<String>,
    last_seen_unix: u64,
}

impl HostBehavioralProfile {
    fn new(now_secs: u64) -> Self {
        Self {
            total_requests: 0,
            post_beacons: 0,
            pixel_or_204_beacons: 0,
            large_bg_uploads: 0,
            silenced_count: 0,
            recent_beacon_ms: VecDeque::with_capacity(32),
            third_party_hosts: BTreeSet::new(),
            called_by_sites: BTreeSet::new(),
            sample_beacon_paths: VecDeque::with_capacity(8),
            last_seen_unix: now_secs,
        }
    }

    fn record_beacon_path(&mut self, path: &str) {
        if path.is_empty() || path == "/" {
            return;
        }
        if !self.sample_beacon_paths.iter().any(|p| p == path) {
            if self.sample_beacon_paths.len() >= 6 {
                self.sample_beacon_paths.pop_back();
            }
            self.sample_beacon_paths.push_front(path.to_string());
        }
    }

    fn compute_trust_of_20(&self, host: &str, burst_10s: usize) -> u8 {
        let mut score: i32 = 20;
        if known_tracker_reason(host).is_some() {
            score -= 8;
        }
        if burst_10s >= BURST_MIN_EVENTS {
            score -= 6;
        } else if self.post_beacons >= 3 {
            score -= 3;
        }
        if self.pixel_or_204_beacons >= 1 {
            score -= 3;
        }
        if self.third_party_hosts.len() >= 6 {
            score -= 4;
        } else if self.third_party_hosts.len() >= 3 {
            score -= 2;
        }
        if self.large_bg_uploads >= 1 {
            score -= 4;
        }
        if self.called_by_sites.len() >= 2 {
            score -= 4; // cross-site tracker seen across multiple domains
        }
        score.clamp(1, 20) as u8
    }
}

pub struct TransparencyRadar {
    rules_path: PathBuf,
    pub blocked_hosts: Arc<RwLock<BTreeSet<String>>>,
    pub silenced_paths: RwLock<BTreeSet<String>>,
    pub auto_silence_beacons: AtomicBool,
    profiles: RwLock<BTreeMap<String, HostBehavioralProfile>>,
    alerts: RwLock<VecDeque<TransparencyAlert>>,
    cooldowns: RwLock<HashMap<(String, String), u64>>,
    next_alert_id: AtomicU64,
    pub total_observations: AtomicU64,
    pub total_alerts: AtomicU64,
    pub micro_beacons_detected: AtomicU64,
    pub pixels_detected: AtomicU64,
    pub beacons_silenced: AtomicU64,
    pub hosts_blocked_hits: AtomicU64,
    alert_tx: broadcast::Sender<TransparencyAlert>,
}

impl TransparencyRadar {
    pub fn load_from_env() -> Arc<Self> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/hue".to_string());
        let cfg_root = std::env::var("AYEFIRE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".config/ayefire"));
        let _ = fs::create_dir_all(&cfg_root);
        let rules_path = cfg_root.join("radar-rules.json");

        let persisted: PersistedRadarRules = fs::read_to_string(&rules_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let (alert_tx, _) = broadcast::channel(128);
        println!(
            "[TRANSPARENCY-RADAR] Zero-float behavioral detector online | blocked_hosts={} | silenced_paths={} | auto_silence={}",
            persisted.blocked_hosts.len(),
            persisted.silenced_paths.len(),
            persisted.auto_silence_beacons
        );

        Arc::new(Self {
            rules_path,
            blocked_hosts: Arc::new(RwLock::new(persisted.blocked_hosts)),
            silenced_paths: RwLock::new(persisted.silenced_paths),
            auto_silence_beacons: AtomicBool::new(persisted.auto_silence_beacons),
            profiles: RwLock::new(BTreeMap::new()),
            alerts: RwLock::new(VecDeque::with_capacity(MAX_ALERT_HISTORY)),
            cooldowns: RwLock::new(HashMap::new()),
            next_alert_id: AtomicU64::new(1),
            total_observations: AtomicU64::new(0),
            total_alerts: AtomicU64::new(0),
            micro_beacons_detected: AtomicU64::new(0),
            pixels_detected: AtomicU64::new(0),
            beacons_silenced: AtomicU64::new(0),
            hosts_blocked_hits: AtomicU64::new(0),
            alert_tx,
        })
    }

    pub fn subscribe_alerts(&self) -> broadcast::Receiver<TransparencyAlert> {
        self.alert_tx.subscribe()
    }

    pub fn shared_blocked_hosts(&self) -> Arc<RwLock<BTreeSet<String>>> {
        Arc::clone(&self.blocked_hosts)
    }

    async fn persist_rules(&self) {
        let rules = PersistedRadarRules {
            blocked_hosts: self.blocked_hosts.read().await.clone(),
            silenced_paths: self.silenced_paths.read().await.clone(),
            auto_silence_beacons: self.auto_silence_beacons.load(Ordering::Relaxed),
        };
        if let Ok(json) = serde_json::to_string_pretty(&rules) {
            let _ = fs::write(&self.rules_path, json);
            let _ = fs::set_permissions(&self.rules_path, fs::Permissions::from_mode(0o600));
        }
    }

    pub async fn is_host_blocked(&self, host: &str) -> bool {
        let low = normalize_host(host);
        let guard = self.blocked_hosts.read().await;
        for blocked in guard.iter() {
            if low == *blocked || low.ends_with(&format!(".{blocked}")) {
                return true;
            }
        }
        false
    }

    pub async fn should_silence_path(&self, host: &str, method: &str, path_only: &str) -> bool {
        let low_host = normalize_host(host);
        let low_path = path_only.to_ascii_lowercase();

        // 1. Explicit user-silenced path rule (`host|path_prefix`)
        {
            let guard = self.silenced_paths.read().await;
            for rule in guard.iter() {
                if let Some((r_host, r_prefix)) = rule.split_once('|') {
                    if low_host == r_host && low_path.starts_with(r_prefix) {
                        return true;
                    }
                }
            }
        }

        // 2. Optional Auto-Silence mode for clear telemetry beacons (never touches HTML pages or auth!)
        if self.auto_silence_beacons.load(Ordering::Relaxed)
            && is_beacon_path(&low_path)
            && (method.eq_ignore_ascii_case("POST")
                || low_path.contains("gen204")
                || low_path.contains("gen_204")
                || low_path.ends_with(".gif"))
        {
            return true;
        }
        false
    }

    pub async fn record_silenced(&self, host: &str, path_only: &str) {
        self.beacons_silenced.fetch_add(1, Ordering::Relaxed);
        let low = normalize_host(host);
        let now_secs = now_unix_secs();
        let mut profs = self.profiles.write().await;
        let entry = profs
            .entry(low)
            .or_insert_with(|| HostBehavioralProfile::new(now_secs));
        entry.silenced_count = entry.silenced_count.saturating_add(1);
        entry.last_seen_unix = now_secs;
        entry.record_beacon_path(path_only);
    }

    pub async fn block_host(&self, host: &str, block: bool) -> bool {
        let clean = normalize_host(host);
        if clean.is_empty() || clean == "localhost" || clean.ends_with(".aye") {
            return false;
        }
        {
            let mut g = self.blocked_hosts.write().await;
            if block {
                g.insert(clean);
            } else {
                g.remove(&clean);
            }
        }
        self.persist_rules().await;
        true
    }

    pub async fn silence_path(&self, host: &str, path_prefix: &str, enable: bool) -> bool {
        let clean_host = normalize_host(host);
        let clean_path = path_prefix.trim().to_ascii_lowercase();
        if clean_host.is_empty() || !clean_path.starts_with('/') || clean_path == "/" {
            return false;
        }
        let rule = format!("{clean_host}|{clean_path}");
        {
            let mut g = self.silenced_paths.write().await;
            if enable {
                g.insert(rule);
            } else {
                g.remove(&rule);
            }
        }
        self.persist_rules().await;
        true
    }

    pub async fn set_auto_silence(&self, enabled: bool) -> bool {
        self.auto_silence_beacons.store(enabled, Ordering::Relaxed);
        self.persist_rules().await;
        enabled
    }

    pub async fn emit_test_alert(&self) -> TransparencyAlert {
        let id = self.next_alert_id.fetch_add(1, Ordering::Relaxed);
        let alert = TransparencyAlert {
            id,
            severity: "info".to_string(),
            kind: "TEST_NOTIFICATION".to_string(),
            host: "radar.aye".to_string(),
            path_sample: "/svc/example/mouse-events".to_string(),
            referer_host: None,
            title: "AyeFire Transparency Radar Notification Active".to_string(),
            detail: "Live SSE & desktop alerts are wired up! When any site fires rapid mouse-move micro-beacons, invisible 1x1 pixels, or cross-site identity trackers, you'll see it here instantly.".to_string(),
            rate_pq: "1/1".to_string(),
            trust_of_20: 20,
            suggested_action: "monitor".to_string(),
            unix_secs: now_unix_secs(),
        };
        self.push_alert(alert.clone()).await;
        alert
    }

    async fn push_alert(&self, alert: TransparencyAlert) {
        self.total_alerts.fetch_add(1, Ordering::Relaxed);
        {
            let mut q = self.alerts.write().await;
            if q.len() >= MAX_ALERT_HISTORY {
                q.pop_back();
            }
            q.push_front(alert.clone());
        }
        let _ = self.alert_tx.send(alert);
    }

    async fn should_emit_with_cooldown(&self, host: &str, kind: &str, now_secs: u64) -> bool {
        let key = (host.to_string(), kind.to_string());
        let mut cd = self.cooldowns.write().await;
        if let Some(&last) = cd.get(&key) {
            if now_secs.saturating_sub(last) < ALERT_COOLDOWN_SECS {
                return false;
            }
        }
        if cd.len() > 512 {
            cd.retain(|_, &mut ts| now_secs.saturating_sub(ts) < ALERT_COOLDOWN_SECS * 2);
        }
        cd.insert(key, now_secs);
        true
    }

    /// Analyze a live inspected request with zero-float integer heuristics and emit calm SSE alerts.
    pub async fn observe_request(&self, obs: TrafficObservation) {
        self.total_observations.fetch_add(1, Ordering::Relaxed);
        let host = normalize_host(&obs.host);
        if host.is_empty() {
            return;
        }
        let now_ms = now_unix_millis();
        let now_secs = now_ms / 1000;
        let low_path = obs.path_only.to_ascii_lowercase();
        let low_ct = obs.content_type.to_ascii_lowercase();
        let is_post_like = obs.method.eq_ignore_ascii_case("POST")
            || obs.method.eq_ignore_ascii_case("PUT");

        let path_is_beacon = is_beacon_path(&low_path);
        let is_small_beacon =
            (is_post_like && obs.resp_bytes <= 256 && (path_is_beacon || obs.req_bytes <= 8192))
                || (obs.status == 204 && obs.resp_bytes == 0);
        let is_tracking_pixel = (low_ct.starts_with("image/gif")
            || low_ct.starts_with("image/png"))
            && obs.resp_bytes > 0
            && obs.resp_bytes <= 128;
        let is_large_bg_upload = is_post_like && path_is_beacon && obs.req_bytes >= 8192;

        let referer_clean = obs
            .referer_host
            .as_deref()
            .map(normalize_host)
            .filter(|r| !r.is_empty() && !same_registrable_site(r, &host));

        // Update bounded host behavioral profiles
        let (burst_10s, trust_of_20, third_party_count, called_by_count, is_new_third_party) = {
            let mut profs = self.profiles.write().await;
            if profs.len() >= MAX_TRACKED_HOSTS && !profs.contains_key(&host) {
                // Evict oldest seen host (C64 bounded RAM)
                if let Some(oldest_k) = profs
                    .iter()
                    .min_by_key(|(_, v)| v.last_seen_unix)
                    .map(|(k, _)| k.clone())
                {
                    profs.remove(&oldest_k);
                }
            }

            let mut ref_tp_count = 0usize;
            let mut added_tp = false;
            if let Some(ref rhost) = referer_clean {
                let r_prof = profs
                    .entry(rhost.clone())
                    .or_insert_with(|| HostBehavioralProfile::new(now_secs));
                r_prof.last_seen_unix = now_secs;
                added_tp = r_prof.third_party_hosts.insert(host.clone());
                ref_tp_count = r_prof.third_party_hosts.len();
            }

            let prof = profs
                .entry(host.clone())
                .or_insert_with(|| HostBehavioralProfile::new(now_secs));
            prof.total_requests = prof.total_requests.saturating_add(1);
            prof.last_seen_unix = now_secs;

            if let Some(ref rhost) = referer_clean {
                prof.called_by_sites.insert(rhost.clone());
            }

            if is_small_beacon || is_tracking_pixel {
                prof.record_beacon_path(&obs.path_only);
                while let Some(&front) = prof.recent_beacon_ms.front() {
                    if now_ms.saturating_sub(front) > BURST_WINDOW_MS {
                        prof.recent_beacon_ms.pop_front();
                    } else {
                        break;
                    }
                }
                if prof.recent_beacon_ms.len() < 32 {
                    prof.recent_beacon_ms.push_back(now_ms);
                }
            }
            if is_small_beacon {
                prof.post_beacons = prof.post_beacons.saturating_add(1);
                self.micro_beacons_detected.fetch_add(1, Ordering::Relaxed);
            }
            if is_tracking_pixel || (obs.status == 204 && obs.resp_bytes == 0) {
                prof.pixel_or_204_beacons = prof.pixel_or_204_beacons.saturating_add(1);
                self.pixels_detected.fetch_add(1, Ordering::Relaxed);
            }
            if is_large_bg_upload {
                prof.large_bg_uploads = prof.large_bg_uploads.saturating_add(1);
            }

            let burst = prof.recent_beacon_ms.len();
            let trust = prof.compute_trust_of_20(&host, burst);
            let called_by = prof.called_by_sites.len();
            (burst, trust, ref_tp_count, called_by, added_tp)
        };

        // --- EVALUATE ALERT RULES ---

        // Alert 1: Rapid Mouse-Move / Scroll Micro-Beacon Burst (>= 4 beacons in 10s!)
        if is_small_beacon
            && burst_10s >= BURST_MIN_EVENTS
            && self
                .should_emit_with_cooldown(&host, "MICRO_BEACON_BURST", now_secs)
                .await
        {
            let rate_pq = format_reduced_pq(burst_10s as u64, 10);
            let id = self.next_alert_id.fetch_add(1, Ordering::Relaxed);
            self.push_alert(TransparencyAlert {
                id,
                severity: "high".to_string(),
                kind: "MICRO_BEACON_BURST".to_string(),
                host: host.clone(),
                path_sample: obs.path_only.clone(),
                referer_host: referer_clean.clone(),
                title: format!("Rapid Background Micro-Beacons on {host}"),
                detail: format!(
                    "{host} fired {burst_10s} small {} packets to {} within 10s ({rate_pq} pkt/s, {} B reply) — typical of mouse-move, hover, or scroll telemetry even without clicking.",
                    obs.method, obs.path_only, obs.resp_bytes
                ),
                rate_pq: format!("{rate_pq} pkt/s"),
                trust_of_20,
                suggested_action: "silence_path".to_string(),
                unix_secs: now_secs,
            })
            .await;
        }

        // Alert 2: Known Identity Broker / Session Replay SDK
        if let Some(reason) = known_tracker_reason(&host) {
            if self
                .should_emit_with_cooldown(&host, "IDENTITY_OR_REPLAY", now_secs)
                .await
            {
                let id = self.next_alert_id.fetch_add(1, Ordering::Relaxed);
                let via_note = referer_clean
                    .as_ref()
                    .map(|r| format!(" (loaded silently while browsing {r})"))
                    .unwrap_or_default();
                self.push_alert(TransparencyAlert {
                    id,
                    severity: "high".to_string(),
                    kind: "IDENTITY_OR_REPLAY".to_string(),
                    host: host.clone(),
                    path_sample: obs.path_only.clone(),
                    referer_host: referer_clean.clone(),
                    title: format!("Tracker / Identity Broker Contacted: {host}"),
                    detail: format!("{reason}{via_note}. Path: {}", obs.path_only),
                    rate_pq: format!("{called_by_count}/1 sites"),
                    trust_of_20,
                    suggested_action: "block_host".to_string(),
                    unix_secs: now_secs,
                })
                .await;
            }
        }

        // Alert 3: Invisible 1x1 Tracking Pixel (<= 128 B GIF/PNG)
        if is_tracking_pixel
            && self
                .should_emit_with_cooldown(&host, "INVISIBLE_PIXEL", now_secs)
                .await
        {
            let id = self.next_alert_id.fetch_add(1, Ordering::Relaxed);
            self.push_alert(TransparencyAlert {
                id,
                severity: "medium".to_string(),
                kind: "INVISIBLE_PIXEL".to_string(),
                host: host.clone(),
                path_sample: obs.path_only.clone(),
                referer_host: referer_clean.clone(),
                title: format!("Invisible {}-Byte Tracking Pixel on {host}", obs.resp_bytes),
                detail: format!(
                    "{host}{} served a tiny {}-byte {} pixel — commonly used to track page views, scroll visibility, and ad attribution.",
                    obs.path_only, obs.resp_bytes, obs.content_type
                ),
                rate_pq: format!("{} B", obs.resp_bytes),
                trust_of_20,
                suggested_action: if referer_clean.is_some() {
                    "block_host".to_string()
                } else {
                    "silence_path".to_string()
                },
                unix_secs: now_secs,
            })
            .await;
        }

        // Alert 4: Silent Background Upload (>= 8 KiB POST to telemetry/replay path)
        if is_large_bg_upload
            && self
                .should_emit_with_cooldown(&host, "SILENT_UPLOAD", now_secs)
                .await
        {
            let id = self.next_alert_id.fetch_add(1, Ordering::Relaxed);
            let kib = obs.req_bytes / 1024;
            self.push_alert(TransparencyAlert {
                id,
                severity: "high".to_string(),
                kind: "SILENT_UPLOAD".to_string(),
                host: host.clone(),
                path_sample: obs.path_only.clone(),
                referer_host: referer_clean.clone(),
                title: format!("Large Silent Telemetry Upload ({kib} KiB) to {host}"),
                detail: format!(
                    "Sent {kib} KiB via {} to {host}{} — size matches DOM snapshot or session-replay upload.",
                    obs.method, obs.path_only
                ),
                rate_pq: format!("{kib} KiB"),
                trust_of_20,
                suggested_action: "silence_path".to_string(),
                unix_secs: now_secs,
            })
            .await;
        }

        // Alert 5: High Third-Party Fanout on a browsing page
        if is_new_third_party && third_party_count >= 6 {
            if let Some(ref rhost) = referer_clean {
                if self
                    .should_emit_with_cooldown(rhost, "THIRD_PARTY_FANOUT", now_secs)
                    .await
                {
                    let id = self.next_alert_id.fetch_add(1, Ordering::Relaxed);
                    self.push_alert(TransparencyAlert {
                        id,
                        severity: "medium".to_string(),
                        kind: "THIRD_PARTY_FANOUT".to_string(),
                        host: rhost.clone(),
                        path_sample: "/".to_string(),
                        referer_host: None,
                        title: format!("{rhost} Quietly Contacted {third_party_count} Third-Party Hosts"),
                        detail: format!(
                            "While browsing {rhost}, your browser was directed to contact {third_party_count} separate external domains (most recently {host})."
                        ),
                        rate_pq: format!("{third_party_count}/1 hosts"),
                        trust_of_20,
                        suggested_action: "monitor".to_string(),
                        unix_secs: now_secs,
                    })
                    .await;
                }
            }
        }
    }

    pub async fn status(&self) -> TransparencyRadarStatus {
        let now_ms = now_unix_millis();
        let blocked_set = self.blocked_hosts.read().await.clone();
        let silenced_vec: Vec<String> = self.silenced_paths.read().await.iter().cloned().collect();
        let alerts_vec: Vec<TransparencyAlert> = self.alerts.read().await.iter().cloned().collect();

        let mut sites = Vec::new();
        {
            let profs = self.profiles.read().await;
            for (host, p) in profs.iter() {
                let burst_10s = p
                    .recent_beacon_ms
                    .iter()
                    .filter(|&&ts| now_ms.saturating_sub(ts) <= BURST_WINDOW_MS)
                    .count();
                let trust = p.compute_trust_of_20(host, burst_10s);
                let verdict = if trust <= 8 {
                    "CHATTY / TRACKING".to_string()
                } else if trust <= 14 {
                    "WATCH (TELEMETRY)".to_string()
                } else {
                    "CALM".to_string()
                };
                let is_blocked = blocked_set
                    .iter()
                    .any(|b| host == b || host.ends_with(&format!(".{b}")));
                sites.push(SiteTransparencyView {
                    host: host.clone(),
                    trust_of_20: trust,
                    trust_pq: format_reduced_pq(trust as u64, 20),
                    verdict,
                    total_requests: p.total_requests,
                    post_beacons: p.post_beacons,
                    pixel_or_204_beacons: p.pixel_or_204_beacons,
                    burst_events_10s: burst_10s,
                    burst_rate_pq: format!("{} pkt/10s", burst_10s),
                    silenced_count: p.silenced_count,
                    third_party_hosts: p.third_party_hosts.iter().cloned().collect(),
                    called_by_sites: p.called_by_sites.iter().cloned().collect(),
                    sample_beacon_paths: p.sample_beacon_paths.iter().cloned().collect(),
                    known_tracker_note: known_tracker_reason(host).map(|s| s.to_string()),
                    is_blocked,
                    last_seen_unix: p.last_seen_unix,
                });
            }
        }
        // Sort chattiest / lowest trust score first, then by beacon volume
        sites.sort_by_key(|s| (s.trust_of_20, std::cmp::Reverse(s.post_beacons + s.pixel_or_204_beacons)));
        sites.truncate(48);

        TransparencyRadarStatus {
            auto_silence_beacons: self.auto_silence_beacons.load(Ordering::Relaxed),
            sites_observed: sites.len(),
            total_observations: self.total_observations.load(Ordering::Relaxed),
            total_alerts: self.total_alerts.load(Ordering::Relaxed),
            micro_beacons_detected: self.micro_beacons_detected.load(Ordering::Relaxed),
            pixels_detected: self.pixels_detected.load(Ordering::Relaxed),
            beacons_silenced: self.beacons_silenced.load(Ordering::Relaxed),
            hosts_blocked_hits: self.hosts_blocked_hits.load(Ordering::Relaxed),
            blocked_hosts: blocked_set.into_iter().collect(),
            silenced_paths: silenced_vec,
            alerts: alerts_vec,
            sites,
        }
    }

    /// Build a strictly sanitized (zero-cookie, zero-PII) prompt for local DeepSeek `p/q` explanation.
    pub async fn build_sanitized_explain_summary(&self, host_query: &str) -> (String, String) {
        let target = normalize_host(host_query);
        let st = self.status().await;
        if let Some(site) = st.sites.iter().find(|s| s.host == target) {
            let paths = if site.sample_beacon_paths.is_empty() {
                "(none)".to_string()
            } else {
                site.sample_beacon_paths.join(", ")
            };
            let tps = if site.third_party_hosts.is_empty() {
                "(none observed)".to_string()
            } else {
                site.third_party_hosts.join(", ")
            };
            let known = site
                .known_tracker_note
                .as_deref()
                .unwrap_or("No known pure-tracker database match");
            let summary = format!(
                "Host: {}\nTrust score: {}/20 ({})\nTotal requests: {}\nSmall POST/204 micro-beacons: {} ({} in last 10s)\nTracking pixels / 204 pings: {}\nSample beacon paths (query strings stripped): {}\nThird-party hosts called from this site: {}\nKnown classification: {}",
                site.host,
                site.trust_of_20,
                site.verdict,
                site.total_requests,
                site.post_beacons,
                site.burst_events_10s,
                site.pixel_or_204_beacons,
                paths,
                tps,
                known
            );
            let fallback = format!(
                "{} currently scores {}/20 ({}) with {} micro-beacons and {} pixel/204 pings (paths: {}). {}. Tip: use 'Silence Path' to drop just the telemetry endpoint without breaking the site, or 'Block Host' for pure third-party trackers.",
                site.host, site.trust_of_20, site.verdict, site.post_beacons, site.pixel_or_204_beacons, paths, known
            );
            (summary, fallback)
        } else {
            let known = known_tracker_reason(&target).unwrap_or("General web host");
            let summary = format!("Host: {target}\nKnown classification: {known}\nNo behavioral burst observed yet in current session.");
            let fallback = format!("{target}: {known}. No rapid micro-beacon bursts recorded yet.");
            (summary, fallback)
        }
    }
}

pub fn known_tracker_reason(host: &str) -> Option<&'static str> {
    let low = normalize_host(host);
    for (suffix, desc) in IDENTITY_OR_REPLAY_HOSTS {
        if low == *suffix || low.ends_with(&format!(".{suffix}")) {
            return Some(desc);
        }
    }
    None
}

pub fn is_beacon_path(path_lower: &str) -> bool {
    BEACON_PATH_HINTS.iter().any(|hint| path_lower.contains(hint))
}

pub fn normalize_host(raw: &str) -> String {
    raw.trim()
        .trim_end_matches('.')
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn extract_referer_host(referer: &str) -> Option<String> {
    let after = referer.split_once("://").map(|(_, r)| r).unwrap_or(referer);
    let host_port = after.split('/').next()?;
    let h = normalize_host(host_port);
    if h.is_empty() {
        None
    } else {
        Some(h)
    }
}

fn same_registrable_site(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let tail = |h: &str| -> String {
        let parts: Vec<&str> = h.split('.').collect();
        if parts.len() >= 2 {
            format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
        } else {
            h.to_string()
        }
    };
    tail(a) == tail(b)
}

fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_unix_secs() -> u64 {
    now_unix_millis() / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn detects_mouse_move_micro_beacon_burst_and_identity_broker() {
        let tmp = std::env::temp_dir().join(format!("ayefire-radar-test-{}", now_unix_millis()));
        let _ = fs::create_dir_all(&tmp);
        let (alert_tx, mut rx) = broadcast::channel(32);
        let radar = TransparencyRadar {
            rules_path: tmp.join("rules.json"),
            blocked_hosts: Arc::new(RwLock::new(BTreeSet::new())),
            silenced_paths: RwLock::new(BTreeSet::new()),
            auto_silence_beacons: AtomicBool::new(false),
            profiles: RwLock::new(BTreeMap::new()),
            alerts: RwLock::new(VecDeque::new()),
            cooldowns: RwLock::new(HashMap::new()),
            next_alert_id: AtomicU64::new(1),
            total_observations: AtomicU64::new(0),
            total_alerts: AtomicU64::new(0),
            micro_beacons_detected: AtomicU64::new(0),
            pixels_detected: AtomicU64::new(0),
            beacons_silenced: AtomicU64::new(0),
            hosts_blocked_hits: AtomicU64::new(0),
            alert_tx,
        };

        for _ in 0..4 {
            radar
                .observe_request(TrafficObservation {
                    host: "www.reddit.com".to_string(),
                    referer_host: None,
                    method: "POST".to_string(),
                    path_only: "/svc/shreddit/events".to_string(),
                    status: 200,
                    req_bytes: 512,
                    resp_bytes: 2,
                    content_type: "text/plain".to_string(),
                })
                .await;
        }
        let alert1 = rx.recv().await.unwrap();
        assert_eq!(alert1.kind, "MICRO_BEACON_BURST");
        assert_eq!(alert1.suggested_action, "silence_path");

        radar
            .observe_request(TrafficObservation {
                host: "id.rlcdn.com".to_string(),
                referer_host: Some("www.reddit.com".to_string()),
                method: "GET".to_string(),
                path_only: "/472486.gif".to_string(),
                status: 200,
                req_bytes: 0,
                resp_bytes: 35,
                content_type: "image/gif".to_string(),
            })
            .await;
        let alert2 = rx.recv().await.unwrap();
        assert_eq!(alert2.kind, "IDENTITY_OR_REPLAY");

        assert!(radar.silence_path("www.reddit.com", "/svc/shreddit/events", true).await);
        assert!(radar.should_silence_path("www.reddit.com", "POST", "/svc/shreddit/events").await);
        assert!(!radar.should_silence_path("www.reddit.com", "GET", "/r/rust").await);

        let _ = fs::remove_dir_all(&tmp);
    }
}
