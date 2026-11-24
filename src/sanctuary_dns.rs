#![deny(clippy::float_arithmetic)]
//! Pure-Rust RFC 1035 Sovereign `.aye` DNS & Offline Captive-Portal Responder (Zero Extra Crates).
//!
//! Resolves local Sanctuary domains (`home.aye`, `library.aye`, `learn.aye`, `ask.aye`,
//! `music.aye`, `kitchen.aye`, `print.aye`, `sanctuary.aye`, and `*.aye`) directly to the
//! AyeFire LAN IPv4 address — enabling a complete offline local Internet with zero WAN.
//!
//! When `offline_portal` is enabled (air-gapped Wi-Fi/LAN appliance mode), OS captive-portal
//! connectivity probes (`captive.apple.com`, `connectivitycheck.gstatic.com`, etc.) also
//! resolve to AyeFire so phones, tablets, and Chromebooks land warmly on `home.aye/welcome`.
//!
//! Zero-surveillance invariant: query names are NEVER logged to disk.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;

pub const DEFAULT_DNS_PORT: u16 = 5354;

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

#[derive(Clone)]
pub struct SanctuaryDnsConfig {
    pub port: u16,
    pub lan_ipv4: Ipv4Addr,
    pub offline_portal: bool,
    pub txt_banner: String,
    pub queries_answered: Arc<AtomicU64>,
}

impl SanctuaryDnsConfig {
    pub fn from_env(doc_count: usize, merkle_short: &str) -> Self {
        let port = std::env::var("AYEFIRE_DNS_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_DNS_PORT);
        let lan_ipv4 = std::env::var("AYEFIRE_LAN_IP")
            .ok()
            .and_then(|s| s.parse::<Ipv4Addr>().ok())
            .unwrap_or_else(detect_lan_ipv4);
        let offline_portal = std::env::var("AYEFIRE_OFFLINE_PORTAL")
            .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
            .unwrap_or(true);
        let txt_banner = format!(
            "AyeFire Sanctuary Internet | Education Understanding Love Respect Space {{<3,<3,<3}}+1 | docs={doc_count} | merkle={merkle_short}"
        );
        Self {
            port,
            lan_ipv4,
            offline_portal,
            txt_banner,
            queries_answered: Arc::new(AtomicU64::new(0)),
        }
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
    let sock = UdpSocket::bind(bind_addr).await?;
    println!(
        "[SANCTUARY-DNS] Sovereign .aye DNS listening on udp://0.0.0.0:{} -> {} (offline_portal={}, zero_query_log=true)",
        cfg.port, cfg.lan_ipv4, cfg.offline_portal
    );

    let mut buf = [0u8; 512];
    loop {
        let (len, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(reply) = build_dns_response(&buf[..len], &cfg) {
            cfg.queries_answered.fetch_add(1, Ordering::Relaxed);
            let _ = sock.send_to(&reply, peer).await;
        }
    }
}

pub fn is_sanctuary_domain(qname_lower: &str, offline_portal: bool) -> bool {
    let name = qname_lower.trim_end_matches('.');
    if name == "aye" || name.ends_with(".aye") {
        return true;
    }
    if offline_portal && CAPTIVE_PORTAL_HOSTS.contains(&name) {
        return true;
    }
    false
}

/// Parse a single RFC 1035 DNS query packet and build a minimal authoritative response.
pub fn build_dns_response(query: &[u8], cfg: &SanctuaryDnsConfig) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    let id_hi = query[0];
    let id_lo = query[1];
    let flags_hi = query[2];
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    // Must be a standard query (QR=0) with at least 1 question
    if (flags_hi & 0x80) != 0 || qdcount == 0 {
        return None;
    }

    let (qname, qend) = parse_qname(query, 12)?;
    if qend + 4 > query.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([query[qend], query[qend + 1]]);
    let qclass = u16::from_be_bytes([query[qend + 2], query[qend + 3]]);
    let question_slice = &query[12..qend + 4];
    let qname_lower = qname.to_ascii_lowercase();
    let rd = flags_hi & 0x01;

    if qclass != QCLASS_IN || !is_sanctuary_domain(&qname_lower, cfg.offline_portal) {
        // NXDOMAIN (RCODE=3)
        let mut resp = Vec::with_capacity(12 + question_slice.len());
        resp.extend_from_slice(&[
            id_hi,
            id_lo,
            0x84 | rd, // QR=1, AA=1, RD echoed
            0x83,      // RA=1, RCODE=3 (NXDOMAIN)
            0x00,
            0x01, // QDCOUNT=1
            0x00,
            0x00, // ANCOUNT=0
            0x00,
            0x00, // NSCOUNT=0
            0x00,
            0x00, // ARCOUNT=0
        ]);
        resp.extend_from_slice(question_slice);
        return Some(resp);
    }

    let mut answers = Vec::new();
    let mut ancount: u16 = 0;

    if qtype == QTYPE_A || qtype == QTYPE_ANY {
        // Name compression pointer to offset 12 (0xC00C)
        answers.extend_from_slice(&[0xC0, 0x0C]);
        answers.extend_from_slice(&QTYPE_A.to_be_bytes());
        answers.extend_from_slice(&QCLASS_IN.to_be_bytes());
        answers.extend_from_slice(&60u32.to_be_bytes()); // TTL 60s
        answers.extend_from_slice(&4u16.to_be_bytes());
        answers.extend_from_slice(&cfg.lan_ipv4.octets());
        ancount = ancount.saturating_add(1);
    }

    if qtype == QTYPE_TXT || qtype == QTYPE_ANY {
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

    // For QTYPE_AAAA (28), return NOERROR with ANCOUNT=0 so browsers immediately use IPv4 A
    let _ = QTYPE_AAAA;

    let mut resp = Vec::with_capacity(12 + question_slice.len() + answers.len());
    resp.extend_from_slice(&[
        id_hi,
        id_lo,
        0x84 | rd, // QR=1, AA=1
        0x80,      // RA=1, RCODE=0 (NOERROR)
        0x00,
        0x01, // QDCOUNT=1
    ]);
    resp.extend_from_slice(&ancount.to_be_bytes());
    resp.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    resp.extend_from_slice(question_slice);
    resp.extend_from_slice(&answers);
    Some(resp)
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
        // Reject compression pointers in question section
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
            0x12, 0x34, // ID
            0x01, 0x00, // Standard query, RD=1
            0x00, 0x01, // QDCOUNT=1
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
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
            offline_portal,
            txt_banner: "AyeFire Sanctuary {<3,<3,<3}+1".to_string(),
            queries_answered: Arc::new(AtomicU64::new(0)),
        }
    }

    #[test]
    fn resolves_aye_a_and_txt_records() {
        let cfg = test_cfg(true);
        let q = make_query("library.aye", QTYPE_A);
        let resp = build_dns_response(&q, &cfg).unwrap();
        assert_eq!(&resp[0..2], &[0x12, 0x34]);
        assert_eq!(resp[3] & 0x0F, 0); // NOERROR
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 1); // 1 answer
        assert_eq!(&resp[resp.len() - 4..], &[172, 16, 47, 118]);

        let qtxt = make_query("home.aye", QTYPE_TXT);
        let rtxt = build_dns_response(&qtxt, &cfg).unwrap();
        assert_eq!(rtxt[3] & 0x0F, 0);
        assert!(String::from_utf8_lossy(&rtxt).contains("AyeFire Sanctuary"));
    }

    #[test]
    fn captive_portal_respects_offline_toggle_and_non_aye_returns_nxdomain() {
        let q_cap = make_query("captive.apple.com", QTYPE_A);
        let r_on = build_dns_response(&q_cap, &test_cfg(true)).unwrap();
        assert_eq!(r_on[3] & 0x0F, 0); // NOERROR -> Sanctuary IP
        assert_eq!(u16::from_be_bytes([r_on[6], r_on[7]]), 1);

        let r_off = build_dns_response(&q_cap, &test_cfg(false)).unwrap();
        assert_eq!(r_off[3] & 0x0F, 3); // NXDOMAIN when offline portal disabled

        let q_ext = make_query("example.com", QTYPE_A);
        let r_ext = build_dns_response(&q_ext, &test_cfg(true)).unwrap();
        assert_eq!(r_ext[3] & 0x0F, 3); // NXDOMAIN
    }
}
