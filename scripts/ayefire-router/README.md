# AyeFire Sovereign Gateway — UniFi Dream Machine (UDM) Retirement Kit

This kit turns `8bsov` (or any dual-NIC Linux box) into your live home edge router, replacing the UniFi Dream Machine cleanly with **zero LAN IP renumbering** and **instant 1-command rollback**.

## Physical Port Mapping (`8bsov`)
- **`eno2` (Intel I226-V 2.5 GbE)** → **WAN** (plug directly into your ISP modem / ONT)
- **`eno1` (Aquantia AQC113 10 GbE)** → **LAN** (plug into your home switch + separate UniFi Wi-Fi APs)
- **`wg-i1`** → Sovereign `i1.is` WireGuard mesh overlay

## Why Your Separate UniFi APs Keep Working Immediately
UniFi Access Points store their SSID, WPA2/WPA3 passphrase, and VLAN config in local flash and keep bridging Wi-Fi to Ethernet **even when the UDM is unplugged**.
Because `ayefire-dnsmasq-dhcp.conf` takes over the exact gateway IP **`172.16.47.67/25`** (while preserving `172.16.47.118/25` as a secondary alias on `eno1`), every existing AP, switch, `s` (`.91`), `ayekvm` (`.103`), and device on `172.16.47.0/25` continues working with zero reconfiguration.

## Step-by-Step Cutover
1. **Verify staged services right now (zero downtime, UDM untouched):**
   ```bash
   ./ayefire-gateway-ctl.sh stage
   ```
2. **Cut over to AyeFire:**
   - Unplug ISP modem WAN cable from the UDM and plug it into `8bsov`'s **Intel 2.5G (`eno2`)** port.
   - Leave `8bsov`'s **10G (`eno1`)** port connected to your LAN switch (and unplug the UDM from the LAN switch so there are no duplicate `.67` gateways).
   - Run:
     ```bash
     sudo ./ayefire-gateway-ctl.sh cutover
     ```
   - `cutover` checks that `eno2` has physical carrier BEFORE changing any routes!
3. **Instant rollback (if ever needed):**
   ```bash
   sudo ./ayefire-gateway-ctl.sh rollback
   ```

## Managing Existing UniFi AP SSIDs Later Without the UDM (Optional)
If you ever want to change Wi-Fi passwords/SSIDs on your separate UniFi APs later, spin up a local LAN-only UniFi Network Application container on demand (stopped when not editing Wi-Fi):
```bash
docker run --rm --name unifi-ap-admin --network host \
  -v ~/.local/share/ayefire/unifi-ap:/config \
  lscr.io/linuxserver/unifi-network-application:latest
```

## Isolating Any Camera / Smart TV to Local-Only (Zero WAN)
Per `ayefire_onboard.rs` Tier 1 ("Nothing leaves by default"):
```bash
sudo nft add element inet ayefire_gateway iot_local_only { 172.16.47.55 }
```
