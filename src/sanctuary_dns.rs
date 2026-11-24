#![deny(clippy::float_arithmetic)]
//! Pure-Rust RFC 1035 Sovereign `.aye` DNS, Caching Forwarding Resolver, Telemetry Sinkhole,
//! and Automatic Offline Captive-Portal Failover (Zero Extra Crates, Zero Query-to-Disk Logging).
//!
//! Modes (`AYEFIRE_DNS_MODE`):
//! - `gateway` (default):
//!   1. Authoritatively answers `*.aye`, `aye`, and `ayefire.local` (`< 1 ms` → AyeFire LAN IPv4).
//!   2. Blocks noisy telemetry/ad/UniFi phone-home domains (`trace.svc.ui.com`, `ping.ui.com`,
//!      `doubleclick.net`, `google-analytics.com`, `telemetry.*`, `metrics.*`) with `0.0.0.0`
//!      (toggleable via `AYEFIRE_DNS_SINKHOLE=1|0`).
//!   3. Checks bounded in-memory TTL cache (`<= 4,096` entries, zero disk logs) → instant LAN hit.
//!   4. Forwards cache misses to upstream DNS (`AYEFIRE_UPSTREAM_DNS`, default `1.1.1.1:53` +
//!      `9.9.9.9:53` fallback) and caches the wire response.
//!   5. **Automatic Air-Gap Failover**: if upstream DNS times out (WAN unplugged!), OS captive
//!      portal probes automatically resolve to AyeFire's LAN IPv4 so devices land cleanly on
//!      `http://home.aye/welcome` until WAN returns!
//! - `airgap`: Never sends any upstream UDP packet; resolves `.aye` + captive probes locally,
//!   returns `NXDOMAIN` for external domains.

use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;

pub const DEFAULT_DNS_PORT: u16 = 5354;
const MAX_DNS_CACHE_ENTRIES: usize = 4_096;
const MIN_CACHE_TTL_SECS: u64 = 60;
const MAX_CACHE_TTL_SECS: u64 = 3_600;

const QTYPE_A: u16 = 1;
const QTYPE_TXT: u16 = 16;
const QTYPE_AAAA: u16 = 28;
const QTYPE_ANY: u16 = 255;
const QCLASS_IN: u16 = 1;

const CAPTIVE_PORTAL_HOSTS: &[&str] = &[
    "captive.apple.com",
    "connectivitycheck.gstatic.com",
    "connectivitycheck.android.com",
    "clients3.google.com",
    "clients1.google.com",
    "detectportal.firefox.com",
    "www.msftconnecttest.com",
    "www.msftncsi.com",
    "msftconnecttest.com",
    "nmcheck.gnome.org",
    "networkcheck.kde.org",
];

/// Built-in zero-telemetry sinkhole suffixes (blocks vendor phone-home & ad beacons).
const TELEMETRY_SINKHOLE_SUFFIXES: &[&str] = &[
    "trace.svc.ui.com",
    "ping.ui.com",
    "crash-report-service.ui.com",
    "doubleclick.net",
    "google-analytics.com",
    "adservice.google.com",
    "googlesyndication.com",
    "scorecardresearch.com",
    "app-measurement.com",
    "events.data.microsoft.com",
    "vortex.data.microsoft.com",
    "telemetry.microsoft.com",
    "metrics.icloud.com",
    "analytics.tiktok.com",
    "pixel.facebook.com",
    "ads.yahoo.com",
];

#[derive(Clone)]
struct CachedDnsWire {
    wire_response: Vec<u8>,
    expires_at: Instant,
}

#[derive(Clone)]
pub struct SanctuaryDnsConfig {
    pub port: u16,
    pub lan_ipv4: Ipv4Addr,
    pub gateway_mode: bool,
    pub offline_portal: bool,
    pub sinkhole_enabled: bool,
    pub upstream_primary: SocketAddr,
    pub upstream_fallback: SocketAddr,
    pub txt_banner: String,
    pub queries_answered: Arc<AtomicU64>,
    pub aye_local_hits: Arc<AtomicU64>,
    pub cache_hits: Arc<AtomicU64>,
    pub upstream_forwards: Arc<AtomicU64>,
    pub sinkhole_blocked: Arc<AtomicU64>,
    pub wan_dns_reachable: Arc<AtomicBool>,
    pub dynamic_blocked_hosts: Arc<RwLock<BTreeSet<String>>>,
    cache: Arc<RwLock<HashMap<(String, u16), CachedDnsWire>>>,
}

impl SanctuaryDnsConfig {
    pub fn from_env(
        doc_count: usize,
        merkle_short: &str,
        dynamic_blocked_hosts: Arc<RwLock<BTreeSet<String>>>,
    ) -> Self {
        let port = std::env::var("AYEFIRE_DNS_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_DNS_PORT);
        let lan_ipv4 = std::env::var("AYEFIRE_LAN_IP")
            .ok()
            .and_then(|s| s.parse::<Ipv4Addr>().ok())
            .unwrap_or_else(detect_lan_ipv4);
        let gateway_mode = std::env::var("AYEFIRE_DNS_MODE")
            .map(|v| !v.eq_ignore_ascii_case("airgap"))
            .unwrap_or(true);
        let offline_portal = std::env::var("AYEFIRE_OFFLINE_PORTAL")
            .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
            .unwrap_or(true);
        let sinkhole_enabled = std::env::var("AYEFIRE_DNS_SINKHOLE")
            .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
            .unwrap_or(true);
        let upstream_primary = std::env::var("AYEFIRE_UPSTREAM_DNS")
            .ok()
            .and_then(|s| s.parse::<SocketAddr>().ok())
            .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53));
        let upstream_fallback = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)), 53);
        let txt_banner = format!(
            "AyeFire Sanctuary Internet | Education Understanding Love Respect Space {{<3,<3,<3}}+1 | docs={doc_count} | merkle={merkle_short}"
        );
        Self {
            port,
            lan_ipv4,
            gateway_mode,
            offline_portal,
            sinkhole_enabled,
            upstream_primary,
            upstream_fallback,
            txt_banner,
            queries_answered: Arc::new(AtomicU64::new(0)),
            aye_local_hits: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
            upstream_forwards: Arc::new(AtomicU64::new(0)),
            sinkhole_blocked: Arc::new(AtomicU64::new(0)),
            wan_dns_reachable: Arc::new(AtomicBool::new(true)),
            dynamic_blocked_hosts,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn cache_len(&self) -> usize {
        self.cache.read().await.len()
    }
}

/// Detect non-loopback LAN IPv4 using a UDP route lookup (no packets actually sent).
pub fn detect_lan_ipv4() -> Ipv4Addr {
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("172.16.47.1:53").is_ok() || sock.connect("192.168.1.1:53").is_ok() {
            if let Ok(SocketAddr::V4(v4)) = sock.local_addr() {
                if !v4.ip().is_loopback() && !v4.ip().is_unspecified() {
                    return *v4.ip();
                }
            }
        }
    }
    Ipv4Addr::new(127, 0, 0, 1)
}

pub async fn spawn_dns_responder(cfg: SanctuaryDnsConfig) -> anyhow::Result<()> {
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), cfg.port);
    let sock = Arc::new(UdpSocket::bind(bind_addr).await?);
    println!(
        "[SANCTUARY-DNS] Sovereign DNS listening on udp://0.0.0.0:{} -> {} (mode={}, sinkhole={}, upstream={}, zero_query_log=true)",
        cfg.port,
        cfg.lan_ipv4,
        if cfg.gateway_mode { "gateway+cache" } else { "airgap" },
        cfg.sinkhole_enabled,
        cfg.upstream_primary
    );

    let mut buf = [0u8; 1024];
    loop {
        let (len, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let pkt = buf[..len].to_vec();
        let sock_clone = Arc::clone(&sock);
        let cfg_clone = cfg.clone();
        tokio::spawn(async move {
            if let Some(reply) = resolve_dns_packet(&pkt, &cfg_clone).await {
                cfg_clone.queries_answered.fetch_add(1, Ordering::Relaxed);
                let _ = sock_clone.send_to(&reply, peer).await;
            }
        });
    }
}

pub fn is_local_aye_domain(qname_lower: &str) -> bool {
    let name = qname_lower.trim_end_matches('.');
    name == "aye" || name.ends_with(".aye") || name == "ayefire.local"
}

pub fn is_captive_portal_host(qname_lower: &str) -> bool {
    let name = qname_lower.trim_end_matches('.');
    CAPTIVE_PORTAL_HOSTS.contains(&name)
}

pub fn is_telemetry_sinkholed(qname_lower: &str) -> bool {
    let name = qname_lower.trim_end_matches('.');
    for suffix in TELEMETRY_SINKHOLE_SUFFIXES {
        if name == *suffix || name.ends_with(&format!(".{suffix}")) {
            return true;
        }
    }
    name.starts_with("telemetry.") || name.starts_with("metrics.")
}

#[cfg(test)]
pub fn is_sanctuary_domain(qname_lower: &str, offline_portal: bool) -> bool {
    if is_local_aye_domain(qname_lower) {
        return true;
    }
    if offline_portal && is_captive_portal_host(qname_lower) {
        return true;
    }
    false
}

/// Full async DNS resolution: local `.aye` → telemetry sinkhole → TTL cache → upstream forward → airgap failover.
pub async fn resolve_dns_packet(query: &[u8], cfg: &SanctuaryDnsConfig) -> Option<Vec<u8>> {
    let parsed = parse_dns_question(query)?;

    // 1. Always answer `.aye` / `ayefire.local` authoritatively in <1ms
    if parsed.qclass == QCLASS_IN && is_local_aye_domain(&parsed.qname_lower) {
        cfg.aye_local_hits.fetch_add(1, Ordering::Relaxed);
        return Some(build_authoritative_reply(query, &parsed, cfg, cfg.lan_ipv4, true));
    }

    // 2. Telemetry / ad / UniFi phone-home + user dynamic Radar blocklist sinkhole
    let dynamic_blocked = {
        let g = cfg.dynamic_blocked_hosts.read().await;
        let qn = parsed.qname_lower.trim_end_matches('.');
        g.iter()
            .any(|b| qn == b || qn.ends_with(&format!(".{b}")))
    };
    if parsed.qclass == QCLASS_IN
        && (dynamic_blocked
            || (cfg.sinkhole_enabled && is_telemetry_sinkholed(&parsed.qname_lower)))
    {
        cfg.sinkhole_blocked.fetch_add(1, Ordering::Relaxed);
        return Some(build_sinkhole_reply(query, &parsed));
    }

    // 3. Air-gap mode or WAN currently unreachable -> land captive portals warmly on home.aye
    let wan_ok = cfg.wan_dns_reachable.load(Ordering::Relaxed);
    if !cfg.gateway_mode || !wan_ok {
        if parsed.qclass == QCLASS_IN
            && cfg.offline_portal
            && is_captive_portal_host(&parsed.qname_lower)
        {
            cfg.aye_local_hits.fetch_add(1, Ordering::Relaxed);
            return Some(build_authoritative_reply(query, &parsed, cfg, cfg.lan_ipv4, false));
        }
        if !cfg.gateway_mode {
            return Some(build_nxdomain_reply(query, &parsed));
        }
    }

    // 4. Bounded in-memory TTL cache lookup (zero disk logging!)
    let cache_key = (parsed.qname_lower.clone(), parsed.qtype);
    {
        let now = Instant::now();
        let guard = cfg.cache.read().await;
        if let Some(entry) = guard.get(&cache_key) {
            if entry.expires_at > now && entry.wire_response.len() >= 12 {
                let mut hit = entry.wire_response.clone();
                hit[0] = query[0];
                hit[1] = query[1];
                cfg.cache_hits.fetch_add(1, Ordering::Relaxed);
                return Some(hit);
            }
        }
    }

    // 5. Forward upstream (primary -> fallback) with strict 1500 ms bound
    match forward_upstream_udp(query, cfg.upstream_primary, cfg.upstream_fallback).await {
        Some(resp) => {
            cfg.wan_dns_reachable.store(true, Ordering::Relaxed);
            cfg.upstream_forwards.fetch_add(1, Ordering::Relaxed);
            let ttl_secs = extract_min_answer_ttl(&resp)
                .unwrap_or(300)
                .clamp(MIN_CACHE_TTL_SECS, MAX_CACHE_TTL_SECS);
            let mut guard = cfg.cache.write().await;
            if guard.len() >= MAX_DNS_CACHE_ENTRIES {
                let now = Instant::now();
                guard.retain(|_, v| v.expires_at > now);
                if guard.len() >= MAX_DNS_CACHE_ENTRIES {
                    // Deterministic clear if still full (C64 bounded RAM)
                    guard.clear();
                }
            }
            guard.insert(
                cache_key,
                CachedDnsWire {
                    wire_response: resp.clone(),
                    expires_at: Instant::now() + Duration::from_secs(ttl_secs),
                },
            );
            Some(resp)
        }
        None => {
            // WAN DNS unreachable -> flip seamlessly into offline Sanctuary captive-portal failover!
            cfg.wan_dns_reachable.store(false, Ordering::Relaxed);
            if parsed.qclass == QCLASS_IN
                && cfg.offline_portal
                && is_captive_portal_host(&parsed.qname_lower)
            {
                return Some(build_authoritative_reply(query, &parsed, cfg, cfg.lan_ipv4, false));
            }
            Some(build_nxdomain_reply(query, &parsed))
        }
    }
}

async fn forward_upstream_udp(
    query: &[u8],
    primary: SocketAddr,
    fallback: SocketAddr,
) -> Option<Vec<u8>> {
    for target in [primary, fallback] {
        let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else {
            continue;
        };
        if sock.send_to(query, target).await.is_err() {
            continue;
        }
        let mut resp = [0u8; 1024];
        match tokio::time::timeout(Duration::from_millis(1500), sock.recv_from(&mut resp)).await {
            Ok(Ok((n, _))) if n >= 12 => {
                let mut out = resp[..n].to_vec();
                out[0] = query[0];
                out[1] = query[1];
                return Some(out);
            }
            _ => continue,
        }
    }
    None
}

/// Synchronous local-only responder kept for unit tests and strict air-gap usage.
#[cfg(test)]
pub fn build_dns_response(query: &[u8], cfg: &SanctuaryDnsConfig) -> Option<Vec<u8>> {
    let parsed = parse_dns_question(query)?;
    if parsed.qclass == QCLASS_IN && cfg.sinkhole_enabled && is_telemetry_sinkholed(&parsed.qname_lower) {
        return Some(build_sinkhole_reply(query, &parsed));
    }
    if parsed.qclass != QCLASS_IN || !is_sanctuary_domain(&parsed.qname_lower, cfg.offline_portal) {
        return Some(build_nxdomain_reply(query, &parsed));
    }
    Some(build_authoritative_reply(query, &parsed, cfg, cfg.lan_ipv4, true))
}

struct ParsedQuestion<'a> {
    qname_lower: String,
    qtype: u16,
    qclass: u16,
    rd: u8,
    question_slice: &'a [u8],
}

fn parse_dns_question(query: &[u8]) -> Option<ParsedQuestion<'_>> {
    if query.len() < 12 {
        return None;
    }
    let flags_hi = query[2];
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if (flags_hi & 0x80) != 0 || qdcount == 0 {
        return None;
    }
    let (qname, qend) = parse_qname(query, 12)?;
    if qend + 4 > query.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([query[qend], query[qend + 1]]);
    let qclass = u16::from_be_bytes([query[qend + 2], query[qend + 3]]);
    Some(ParsedQuestion {
        qname_lower: qname.to_ascii_lowercase(),
        qtype,
        qclass,
        rd: flags_hi & 0x01,
        question_slice: &query[12..qend + 4],
    })
}

fn build_nxdomain_reply(query: &[u8], q: &ParsedQuestion<'_>) -> Vec<u8> {
    let mut resp = Vec::with_capacity(12 + q.question_slice.len());
    resp.extend_from_slice(&[
        query[0],
        query[1],
        0x84 | q.rd,
        0x83, // RA=1, RCODE=3 (NXDOMAIN)
        0x00,
        0x01,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
    ]);
    resp.extend_from_slice(q.question_slice);
    resp
}

fn build_sinkhole_reply(query: &[u8], q: &ParsedQuestion<'_>) -> Vec<u8> {
    let mut answers = Vec::new();
    let mut ancount: u16 = 0;
    if q.qtype == QTYPE_A || q.qtype == QTYPE_ANY {
        answers.extend_from_slice(&[0xC0, 0x0C]);
        answers.extend_from_slice(&QTYPE_A.to_be_bytes());
        answers.extend_from_slice(&QCLASS_IN.to_be_bytes());
        answers.extend_from_slice(&300u32.to_be_bytes());
        answers.extend_from_slice(&4u16.to_be_bytes());
        answers.extend_from_slice(&[0, 0, 0, 0]); // 0.0.0.0 sinkhole
        ancount = 1;
    }
    let mut resp = Vec::with_capacity(12 + q.question_slice.len() + answers.len());
    resp.extend_from_slice(&[query[0], query[1], 0x84 | q.rd, 0x80, 0x00, 0x01]);
    resp.extend_from_slice(&ancount.to_be_bytes());
    resp.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    resp.extend_from_slice(q.question_slice);
    resp.extend_from_slice(&answers);
    resp
}

fn build_authoritative_reply(
    query: &[u8],
    q: &ParsedQuestion<'_>,
    cfg: &SanctuaryDnsConfig,
    ip: Ipv4Addr,
    include_txt: bool,
) -> Vec<u8> {
    let mut answers = Vec::new();
    let mut ancount: u16 = 0;

    if q.qtype == QTYPE_A || q.qtype == QTYPE_ANY {
        answers.extend_from_slice(&[0xC0, 0x0C]);
        answers.extend_from_slice(&QTYPE_A.to_be_bytes());
        answers.extend_from_slice(&QCLASS_IN.to_be_bytes());
        answers.extend_from_slice(&60u32.to_be_bytes());
        answers.extend_from_slice(&4u16.to_be_bytes());
        answers.extend_from_slice(&ip.octets());
        ancount = ancount.saturating_add(1);
    }

    if include_txt && (q.qtype == QTYPE_TXT || q.qtype == QTYPE_ANY) {
        let txt_bytes = cfg.txt_banner.as_bytes();
        let chunk_len = txt_bytes.len().min(255);
        let rdata_len = (chunk_len + 1) as u16;
        answers.extend_from_slice(&[0xC0, 0x0C]);
        answers.extend_from_slice(&QTYPE_TXT.to_be_bytes());
        answers.extend_from_slice(&QCLASS_IN.to_be_bytes());
        answers.extend_from_slice(&60u32.to_be_bytes());
        answers.extend_from_slice(&rdata_len.to_be_bytes());
        answers.push(chunk_len as u8);
        answers.extend_from_slice(&txt_bytes[..chunk_len]);
        ancount = ancount.saturating_add(1);
    }

    let _ = QTYPE_AAAA;

    let mut resp = Vec::with_capacity(12 + q.question_slice.len() + answers.len());
    resp.extend_from_slice(&[query[0], query[1], 0x84 | q.rd, 0x80, 0x00, 0x01]);
    resp.extend_from_slice(&ancount.to_be_bytes());
    resp.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    resp.extend_from_slice(q.question_slice);
    resp.extend_from_slice(&answers);
    resp
}

fn extract_min_answer_ttl(resp: &[u8]) -> Option<u64> {
    if resp.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([resp[4], resp[5]]) as usize;
    let ancount = u16::from_be_bytes([resp[6], resp[7]]) as usize;
    if ancount == 0 {
        return None;
    }
    let mut pos = 12usize;
    for _ in 0..qdcount {
        pos = skip_dns_name(resp, pos)?;
        pos = pos.checked_add(4)?;
        if pos > resp.len() {
            return None;
        }
    }
    let mut min_ttl: Option<u64> = None;
    for _ in 0..ancount {
        pos = skip_dns_name(resp, pos)?;
        if pos + 10 > resp.len() {
            break;
        }
        let ttl = u32::from_be_bytes([resp[pos + 4], resp[pos + 5], resp[pos + 6], resp[pos + 7]])
            as u64;
        let rdlen = u16::from_be_bytes([resp[pos + 8], resp[pos + 9]]) as usize;
        min_ttl = Some(match min_ttl {
            Some(cur) => cur.min(ttl),
            None => ttl,
        });
        pos = pos.checked_add(10)?.checked_add(rdlen)?;
    }
    min_ttl
}

fn skip_dns_name(pkt: &[u8], mut pos: usize) -> Option<usize> {
    let mut steps = 0usize;
    while pos < pkt.len() {
        steps += 1;
        if steps > 64 {
            return None;
        }
        let len = pkt[pos];
        if len == 0 {
            return Some(pos + 1);
        }
        if (len & 0xC0) == 0xC0 {
            return Some(pos + 2);
        }
        pos = pos.checked_add(1)?.checked_add(len as usize)?;
    }
    None
}

fn parse_qname(pkt: &[u8], start: usize) -> Option<(String, usize)> {
    let mut pos = start;
    let mut labels: Vec<&str> = Vec::new();
    let mut steps = 0usize;
    while pos < pkt.len() {
        steps += 1;
        if steps > 64 {
            return None;
        }
        let len = pkt[pos] as usize;
        pos += 1;
        if len == 0 {
            return Some((labels.join("."), pos));
        }
        if (len & 0xC0) != 0 || pos + len > pkt.len() {
            return None;
        }
        let label = std::str::from_utf8(&pkt[pos..pos + len]).ok()?;
        labels.push(label);
        pos += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_query(domain: &str, qtype: u16) -> Vec<u8> {
        let mut pkt = vec![
            0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        for label in domain.split('.') {
            pkt.push(label.len() as u8);
            pkt.extend_from_slice(label.as_bytes());
        }
        pkt.push(0x00);
        pkt.extend_from_slice(&qtype.to_be_bytes());
        pkt.extend_from_slice(&QCLASS_IN.to_be_bytes());
        pkt
    }

    fn test_cfg(offline_portal: bool) -> SanctuaryDnsConfig {
        SanctuaryDnsConfig {
            port: DEFAULT_DNS_PORT,
            lan_ipv4: Ipv4Addr::new(172, 16, 47, 118),
            gateway_mode: false,
            offline_portal,
            sinkhole_enabled: true,
            upstream_primary: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53),
            upstream_fallback: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)), 53),
            txt_banner: "AyeFire Sanctuary {<3,<3,<3}+1".to_string(),
            queries_answered: Arc::new(AtomicU64::new(0)),
            aye_local_hits: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
            upstream_forwards: Arc::new(AtomicU64::new(0)),
            sinkhole_blocked: Arc::new(AtomicU64::new(0)),
            wan_dns_reachable: Arc::new(AtomicBool::new(true)),
            dynamic_blocked_hosts: Arc::new(RwLock::new(BTreeSet::new())),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[test]
    fn resolves_aye_a_and_txt_records() {
        let cfg = test_cfg(true);
        let q = make_query("library.aye", QTYPE_A);
        let resp = build_dns_response(&q, &cfg).unwrap();
        assert_eq!(&resp[0..2], &[0x12, 0x34]);
        assert_eq!(resp[3] & 0x0F, 0);
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 1);
        assert_eq!(&resp[resp.len() - 4..], &[172, 16, 47, 118]);

        let qtxt = make_query("home.aye", QTYPE_TXT);
        let rtxt = build_dns_response(&qtxt, &cfg).unwrap();
        assert_eq!(rtxt[3] & 0x0F, 0);
        assert!(String::from_utf8_lossy(&rtxt).contains("AyeFire Sanctuary"));
    }

    #[test]
    fn sinkholes_unifi_and_ad_telemetry_cleanly() {
        let cfg = test_cfg(true);
        let q = make_query("trace.svc.ui.com", QTYPE_A);
        let resp = build_dns_response(&q, &cfg).unwrap();
        assert_eq!(resp[3] & 0x0F, 0); // NOERROR
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 1);
        assert_eq!(&resp[resp.len() - 4..], &[0, 0, 0, 0]); // Sinkholed to 0.0.0.0
    }

    #[test]
    fn captive_portal_respects_offline_toggle_and_non_aye_returns_nxdomain() {
        let q_cap = make_query("captive.apple.com", QTYPE_A);
        let r_on = build_dns_response(&q_cap, &test_cfg(true)).unwrap();
        assert_eq!(r_on[3] & 0x0F, 0);
        assert_eq!(u16::from_be_bytes([r_on[6], r_on[7]]), 1);

        let r_off = build_dns_response(&q_cap, &test_cfg(false)).unwrap();
        assert_eq!(r_off[3] & 0x0F, 3);

        let q_ext = make_query("example.com", QTYPE_A);
        let r_ext = build_dns_response(&q_ext, &test_cfg(true)).unwrap();
        assert_eq!(r_ext[3] & 0x0F, 3);
    }
}
