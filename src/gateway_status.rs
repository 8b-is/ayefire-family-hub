#![deny(clippy::float_arithmetic)]
//! AyeFire Live Hardware Gateway Telemetry (Zero-Float Kernel Inspection).
//!
//! Reads real Linux `/sys/class/net`, `/proc/net/route`, `/proc/net/arp`, and
//! `/proc/sys/net/ipv4/ip_forward` state directly — never fabricating link status,
//! throughput, or UDM cutover state.

use serde::Serialize;
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct NicHardwareView {
    pub name: String,
    pub role: String,
    pub operstate: String,
    pub carrier: bool,
    pub speed_mbps: i64,
    pub mac: String,
    pub rx_mib: u64,
    pub tx_mib: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct LanNeighborView {
    pub ip: String,
    pub mac: String,
    pub device: String,
    pub is_current_gateway: bool,
    pub vendor_hint: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GatewayHardwareSnapshot {
    pub mode: String,
    pub ip_forward_enabled: bool,
    pub default_gateway_ip: String,
    pub default_gateway_iface: String,
    pub udm_still_active: bool,
    pub wan_nic_ready: bool,
    pub cutover_script_installed: bool,
    pub interfaces: Vec<NicHardwareView>,
    pub lan_neighbors: Vec<LanNeighborView>,
}

pub fn inspect_gateway_hardware() -> GatewayHardwareSnapshot {
    let ip_forward_enabled = fs::read_to_string("/proc/sys/net/ipv4/ip_forward")
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    let (default_gateway_iface, default_gateway_ip) = read_default_gateway();

    let watched = [
        ("eno1", "10 GbE LAN (Aquantia AQC113 → Home Switch & APs)"),
        ("eno2", "2.5 GbE WAN (Intel I226-V → ISP Modem Cutover Port)"),
        ("wlan0", "Wi-Fi 7 802.11be (Intel BE200 / Backup AP)"),
        ("wg-i1", "Sovereign i1.is WireGuard Mesh Overlay"),
    ];

    let mut interfaces = Vec::new();
    let mut wan_nic_ready = false;
    for (name, role) in watched {
        let base = format!("/sys/class/net/{name}");
        if !Path::new(&base).exists() {
            continue;
        }
        let operstate = read_trim(&format!("{base}/operstate")).unwrap_or_else(|| "unknown".into());
        let carrier = read_trim(&format!("{base}/carrier"))
            .map(|v| v == "1")
            .unwrap_or(false);
        let speed_mbps = read_trim(&format!("{base}/speed"))
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(-1);
        let mac = read_trim(&format!("{base}/address")).unwrap_or_else(|| "—".into());
        let rx_bytes = read_trim(&format!("{base}/statistics/rx_bytes"))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let tx_bytes = read_trim(&format!("{base}/statistics/tx_bytes"))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        if name == "eno2" && carrier {
            wan_nic_ready = true;
        }
        interfaces.push(NicHardwareView {
            name: name.to_string(),
            role: role.to_string(),
            operstate,
            carrier,
            speed_mbps,
            mac,
            rx_mib: rx_bytes / (1024 * 1024),
            tx_mib: tx_bytes / (1024 * 1024),
        });
    }

    let lan_neighbors = read_arp_neighbors(&default_gateway_ip);
    let udm_still_active = lan_neighbors
        .iter()
        .any(|n| n.is_current_gateway && n.vendor_hint.contains("Ubiquiti"));

    let mode = if ip_forward_enabled && default_gateway_iface == "eno2" {
        "LIVE GATEWAY (eno2 WAN → eno1 10G LAN)".to_string()
    } else if udm_still_active {
        format!("STAGED PARALLEL (UDM {default_gateway_ip} currently routing — eno2 ready for cutover)")
    } else {
        format!("STAGED LAN NODE (gateway {default_gateway_ip} via {default_gateway_iface})")
    };

    let cutover_script_installed = Path::new(
        "/home/hue/source/8b-is/ayefire-family-hub/scripts/ayefire-router/ayefire-gateway-ctl.sh",
    )
    .exists()
        || Path::new("/home/hue/.local/bin/ayefire-gateway-ctl").exists();

    GatewayHardwareSnapshot {
        mode,
        ip_forward_enabled,
        default_gateway_ip,
        default_gateway_iface,
        udm_still_active,
        wan_nic_ready,
        cutover_script_installed,
        interfaces,
        lan_neighbors,
    }
}

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_default_gateway() -> (String, String) {
    let Ok(raw) = fs::read_to_string("/proc/net/route") else {
        return ("unknown".to_string(), "0.0.0.0".to_string());
    };
    for line in raw.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 3 && cols[1] == "00000000" {
            if let Ok(gw_le) = u32::from_str_radix(cols[2], 16) {
                let b = gw_le.to_le_bytes();
                let ip = Ipv4Addr::new(b[0], b[1], b[2], b[3]);
                return (cols[0].to_string(), ip.to_string());
            }
        }
    }
    ("none".to_string(), "0.0.0.0".to_string())
}

fn read_arp_neighbors(default_gw_ip: &str) -> Vec<LanNeighborView> {
    let Ok(raw) = fs::read_to_string("/proc/net/arp") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in raw.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 6 {
            let ip = cols[0].to_string();
            let mac = cols[3].to_ascii_lowercase();
            let dev = cols[5].to_string();
            if mac == "00:00:00:00:00:00" {
                continue;
            }
            let vendor_hint = classify_mac_vendor(&mac);
            out.push(LanNeighborView {
                is_current_gateway: ip == default_gw_ip,
                ip,
                mac,
                device: dev,
                vendor_hint,
            });
        }
    }
    out
}

fn classify_mac_vendor(mac: &str) -> String {
    if mac.starts_with("70:a7:41")
        || mac.starts_with("fc:ec:da")
        || mac.starts_with("24:5a:4c")
        || mac.starts_with("74:83:c2")
        || mac.starts_with("e4:38:83")
    {
        "Ubiquiti / UniFi".to_string()
    } else if mac.starts_with("b8:27:eb")
        || mac.starts_with("dc:a6:32")
        || mac.starts_with("e4:5f:01")
        || mac.starts_with("2c:cf:67")
    {
        "Raspberry Pi".to_string()
    } else {
        "LAN Device".to_string()
    }
}
