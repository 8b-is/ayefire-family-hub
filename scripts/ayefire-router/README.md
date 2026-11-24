# AyeFire Sovereign Multi-VLAN Gateway — UDM-SE Retirement Kit

Turns a dual-NIC Linux box (e.g., `8bsov` with `eno2` 2.5 GbE WAN + `eno1` 10 GbE LAN trunk) into your live home edge router — supporting **ISP WAN MAC cloning**, **full 802.1Q multi-VLAN DHCP + routing**, **camera/IoT zero-WAN isolation**, **transparent port-53 AyeFire DNS steering**, and **instant 1-command rollback**.

## Private Site Configuration (Never Committed to Public Git)
Store your site-specific ISP cloned MAC address, VLAN gateway map, and static DHCP reservations in mode `0600` files outside the repo:
- `~/.config/ayefire/site.env` (or `/etc/ayefire/site.env`)
- `~/.config/ayefire/dhcp-site.conf` (or `/etc/ayefire/dhcp-site.conf`)

Example `site.env`:
```bash
AYEFIRE_WAN_IF="eno2"
AYEFIRE_WAN_CLONE_MAC="xx:xx:xx:xx:xx:xx"
AYEFIRE_LAN_IF="eno1"
AYEFIRE_LAN_GW_CIDR="172.16.47.67/25"
AYEFIRE_LAN_ALIAS_CIDR="172.16.47.118/25"
AYEFIRE_LAN_VLANS="1:192.168.1.254/24 43:192.168.43.1/24 255:192.168.255.254/25 363:192.168.253.254/24"
```

## Cutover Steps
1. **Preflight & staged verification (UDM stays live):**
   ```bash
   ayefire-gateway-ctl stage
   ```
2. **Switch trunk check (before unplugging UDM):**
   - Ensure the switch port connected to `eno1` (10 GbE) carries **Native VLAN 47 + Tagged ALL VLANs** (or move the existing 10G switch trunk cable straight onto `eno1`).
   - Move any PoE devices plugged directly into the UDM-SE's built-in 8-port PoE switch (`eth0`–`eth7`) over to your PoE switches (`USW-Pro-24-PoE` / `US-16-PoE-150W` / `US-8-150W`).
3. **Cut over:**
   - Move the ISP ONT/modem Ethernet cable → `eno2` (Intel 2.5G).
   - Unplug the UDM from the LAN trunk so there are no duplicate gateway IPs.
   - Run:
     ```bash
     sudo ayefire-gateway-ctl cutover
     ```
   - `cutover` verifies `eno2` physical link carrier BEFORE modifying any routes, clones your ISP WAN MAC onto `eno2`, brings up all 802.1Q VLANs, enforces zero-WAN camera isolation, steers DNS to AyeFire `:5354`, and starts DHCP.
4. **Instant rollback anytime:**
   ```bash
   sudo ayefire-gateway-ctl rollback
   ```
