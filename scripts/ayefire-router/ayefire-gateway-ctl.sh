#!/usr/bin/env bash
# AyeFire Sovereign Multi-VLAN Gateway Cutover Controller (UDM-SE -> AyeFire)
#
# Supports:
#   - ISP WAN MAC Address Cloning (`AYEFIRE_WAN_CLONE_MAC`) so fiber ONT / cable modem
#     hands AyeFire the exact same WAN DHCP lease instantaneously with zero ISP wait!
#   - Full 802.1Q multi-VLAN LAN trunking (`AYEFIRE_LAN_VLANS="vid:ip/prefix ..."`)
#   - Site-private 0600 config auto-load (`~/.config/ayefire/site.env` + `dhcp-site.conf`)
#   - Instant 1-command rollback back to normal LAN DHCP client!
#
# Usage:
#   ayefire-gateway-ctl {preflight|stage|cutover|rollback|status}

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
REAL_HOME="${SUDO_USER:+$(getent passwd "$SUDO_USER" | cut -d: -f6)}"
REAL_HOME="${REAL_HOME:-$HOME}"

# Auto-load private site config (MAC clone, multi-VLAN map, local DHCP reservations) if present
SITE_ENV=""
for cand in "/etc/ayefire/site.env" "$REAL_HOME/.config/ayefire/site.env"; do
    if [[ -f "$cand" ]]; then
        SITE_ENV="$cand"
        # shellcheck disable=SC1090
        source "$cand"
        break
    fi
done

WAN_IF="${AYEFIRE_WAN_IF:-eno2}"
WAN_CLONE_MAC="${AYEFIRE_WAN_CLONE_MAC:-}"
LAN_IF="${AYEFIRE_LAN_IF:-eno1}"
LAN_GW_CIDR="${AYEFIRE_LAN_GW_CIDR:-172.16.47.67/25}"
LAN_ALIAS_CIDR="${AYEFIRE_LAN_ALIAS_CIDR:-172.16.47.118/25}"
# Space-separated "VID:IP/PREFIX" pairs for 802.1Q tagged VLANs on $LAN_IF
LAN_VLANS="${AYEFIRE_LAN_VLANS:-}"
NFT_CONF="$SCRIPT_DIR/ayefire-nftables.conf"
DHCP_CONF="$SCRIPT_DIR/ayefire-dnsmasq-dhcp.conf"
for cand in "/etc/ayefire/dhcp-site.conf" "$REAL_HOME/.config/ayefire/dhcp-site.conf"; do
    if [[ -f "$cand" ]]; then
        DHCP_CONF="$cand"
        break
    fi
done
DHCP_PID="/run/ayefire-dnsmasq.pid"

need_root() {
    if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
        echo "[ERROR] '$1' requires root privileges. Run: sudo $0 $1" >&2
        exit 1
    fi
}

cmd_preflight() {
    echo "=== AyeFire Multi-VLAN Gateway Preflight Check ==="
    echo "  Site env   : ${SITE_ENV:-(default template)}"
    echo "  DHCP conf  : $DHCP_CONF"
    echo "  WAN port   : $WAN_IF (clone MAC: ${WAN_CLONE_MAC:-(hardware default)})"
    echo "  LAN trunk  : $LAN_IF ($LAN_GW_CIDR + alias $LAN_ALIAS_CIDR)"
    if [[ -n "$LAN_VLANS" ]]; then
        echo "  802.1Q VLANs: $LAN_VLANS"
    fi
    echo "--- Physical Interfaces ---"
    for iface in "$LAN_IF" "$WAN_IF" wlan0 wg-i1; do
        if [[ -d "/sys/class/net/$iface" ]]; then
            state="$(cat "/sys/class/net/$iface/operstate" 2>/dev/null || echo unknown)"
            carrier="$(cat "/sys/class/net/$iface/carrier" 2>/dev/null || echo 0)"
            speed="$(cat "/sys/class/net/$iface/speed" 2>/dev/null || echo -1)"
            mac="$(cat "/sys/class/net/$iface/address" 2>/dev/null || echo unknown)"
            printf "  %-6s  state=%-8s carrier=%s  speed=%-6s Mbps  mac=%s\n" "$iface" "$state" "$carrier" "$speed" "$mac"
        fi
    done
    echo "--- Default route ---"
    ip route show default || true
    echo "--- Active AyeFire local listeners ---"
    ss -tulpn 2>/dev/null | grep -E ':(5354|6631|8080|8790|8792)\b' || echo "  (none detected — start ayefire-family-hub.service)"
    echo "--- Tools ---"
    for bin in nft dnsmasq nmcli ip; do
        command -v "$bin" >/dev/null && echo "  [OK] $bin -> $(command -v "$bin")" || echo "  [MISSING] $bin"
    done
    echo "--- Config syntax check ---"
    if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
        nft -c -f "$NFT_CONF" && echo "  [OK] $NFT_CONF syntax valid"
    else
        echo "  [INFO] Run with sudo to dry-check nftables rules ($NFT_CONF)"
    fi
    dnsmasq --test -C "$DHCP_CONF" 2>&1 | sed 's/^/  /'
}

cmd_stage() {
    echo "=== AyeFire Staged Parallel Verification (UDM stays untouched) ==="
    cmd_preflight
    echo ""
    echo "Staged endpoints you can test RIGHT NOW (zero UDM disruption):"
    echo "  • AyeFire Hub / Sanctuary / Phoenix : http://172.16.47.118:8790/"
    echo "  • Sovereign .aye + Caching DNS      : 172.16.47.118 (UDP port 5354)"
    echo "  • BLAKE3 LAN HTTP Cache Proxy       : http://172.16.47.118:8792"
    echo "  • Print-to-Aye IPP Printer          : ipp://172.16.47.118:6631/ipp/print"
    echo ""
    echo "Cutover checklist when ready to retire the UDM-SE:"
    echo "  1. Ensure the switch port feeding '$LAN_IF' (10G) carries Native VLAN 47 + Tagged ALL VLANs"
    echo "     (or plug the USW-Pro-24-PoE 10G uplink cable directly into '$LAN_IF')."
    echo "  2. Move the ISP ONT/modem cable from UDM-SE eth8 -> 8bsov '$WAN_IF' (Intel 2.5G)."
    echo "  3. Disconnect UDM-SE from the LAN trunk (so there is no duplicate gateway on .67)."
    echo "  4. Run: sudo $0 cutover"
}

cmd_cutover() {
    need_root cutover
    echo "=== AyeFire Live Multi-VLAN Gateway Cutover (Retiring UDM-SE) ==="

    # 1. Verify WAN interface has physical Ethernet link BEFORE modifying any LAN/WAN route!
    ip link set "$WAN_IF" up
    sleep 1
    carrier="$(cat "/sys/class/net/$WAN_IF/carrier" 2>/dev/null || echo 0)"
    if [[ "$carrier" != "1" ]]; then
        echo "[ABORT] No physical Ethernet link detected on WAN interface '$WAN_IF' (carrier=$carrier)!" >&2
        echo "        Plug your ISP ONT/modem into '$WAN_IF' (Intel 2.5G port) first, then re-run cutover." >&2
        exit 2
    fi

    # 2. Configure WAN DHCP on eno2 WITH ISP MAC address clone if configured
    echo "[1/7] Configuring WAN ($WAN_IF) with cloned MAC '${WAN_CLONE_MAC:-hardware}'..."
    if nmcli -t -f NAME con show | grep -qx "ayefire-wan"; then
        if [[ -n "$WAN_CLONE_MAC" ]]; then
            nmcli con mod ayefire-wan 802-3-ethernet.cloned-mac-address "$WAN_CLONE_MAC" ipv4.method auto ipv6.method auto
        else
            nmcli con mod ayefire-wan ipv4.method auto ipv6.method auto
        fi
        nmcli con up ayefire-wan
    else
        if [[ -n "$WAN_CLONE_MAC" ]]; then
            nmcli con add type ethernet ifname "$WAN_IF" con-name ayefire-wan \
                802-3-ethernet.cloned-mac-address "$WAN_CLONE_MAC" \
                ipv4.method auto ipv6.method auto
        else
            nmcli con add type ethernet ifname "$WAN_IF" con-name ayefire-wan \
                ipv4.method auto ipv6.method auto
        fi
        nmcli con up ayefire-wan
    fi
    echo "  WAN $WAN_IF active MAC: $(cat "/sys/class/net/$WAN_IF/address")"

    # 3. Assign exact native VLAN 47 gateway IP (172.16.47.67/25) + preserve .118 alias on eno1
    echo "[2/7] Configuring native LAN ($LAN_IF) as $LAN_GW_CIDR (preserving $LAN_ALIAS_CIDR)..."
    if nmcli -t -f NAME con show | grep -qx "ayefire-lan"; then
        nmcli con mod ayefire-lan ipv4.method manual ipv4.addresses "$LAN_GW_CIDR,$LAN_ALIAS_CIDR" ipv4.never-default yes
        nmcli con up ayefire-lan
    else
        nmcli con add type ethernet ifname "$LAN_IF" con-name ayefire-lan \
            ipv4.method manual ipv4.addresses "$LAN_GW_CIDR,$LAN_ALIAS_CIDR" ipv4.never-default yes
        nmcli con up ayefire-lan
    fi

    # 4. Bring up all configured 802.1Q tagged VLAN subinterfaces on eno1
    echo "[3/7] Creating 802.1Q tagged VLAN subinterfaces on $LAN_IF..."
    modprobe 8021q 2>/dev/null || true
    for entry in $LAN_VLANS; do
        vid="${entry%%:*}"
        cidr="${entry#*:}"
        vdev="${LAN_IF}.${vid}"
        if ! ip link show "$vdev" >/dev/null 2>&1; then
            ip link add link "$LAN_IF" name "$vdev" type vlan id "$vid"
        fi
        ip addr replace "$cidr" dev "$vdev"
        ip link set "$vdev" up
        echo "  [OK] $vdev (VLAN $vid) -> $cidr"
    done

    # 5. Enable kernel IPv4/IPv6 packet forwarding
    echo "[4/7] Enabling Linux kernel IP forwarding..."
    sysctl -w net.ipv4.ip_forward=1 >/dev/null
    sysctl -w net.ipv6.conf.all.forwarding=1 >/dev/null
    cat >/etc/sysctl.d/90-ayefire-gateway.conf <<'SYSCTL'
net.ipv4.ip_forward = 1
net.ipv6.conf.all.forwarding = 1
SYSCTL

    # 6. Load stateful multi-VLAN nftables firewall + NAT + port-53 DNS steering
    echo "[5/7] Loading stateful AyeFire nftables firewall & NAT ($NFT_CONF)..."
    nft -f "$NFT_CONF"

    # 7. Start multi-VLAN DHCPv4 server
    echo "[6/7] Starting AyeFire multi-VLAN DHCPv4 server ($DHCP_CONF)..."
    mkdir -p /var/lib/ayefire
    if [[ -f "$DHCP_PID" ]]; then
        kill "$(cat "$DHCP_PID")" 2>/dev/null || true
        rm -f "$DHCP_PID"
    fi
    dnsmasq -C "$DHCP_CONF" --pid-file="$DHCP_PID"

    echo "[7/7] Live verification:"
    ip -br addr show "$WAN_IF" "$LAN_IF"
    for entry in $LAN_VLANS; do
        vid="${entry%%:*}"
        ip -br addr show "${LAN_IF}.${vid}" 2>/dev/null || true
    done
    ip route show default
    echo "[DONE] AyeFire is now your live multi-VLAN edge router with cloned WAN MAC!"
}

cmd_rollback() {
    need_root rollback
    echo "=== Rolling back AyeFire Gateway -> Standard UDM LAN Client ==="
    if [[ -f "$DHCP_PID" ]]; then
        kill "$(cat "$DHCP_PID")" 2>/dev/null || true
        rm -f "$DHCP_PID"
    fi
    nft delete table inet ayefire_gateway 2>/dev/null || true
    sysctl -w net.ipv4.ip_forward=0 >/dev/null || true
    rm -f /etc/sysctl.d/90-ayefire-gateway.conf
    for entry in $LAN_VLANS; do
        vid="${entry%%:*}"
        ip link del "${LAN_IF}.${vid}" 2>/dev/null || true
    done
    nmcli con down ayefire-wan 2>/dev/null || true
    nmcli con down ayefire-lan 2>/dev/null || true
    nmcli con up "Wired connection 1" || nmcli dev connect "$LAN_IF"
    echo "[DONE] Restored $LAN_IF to standard DHCP client. Plug the UDM-SE back in if needed."
}

cmd_status() {
    echo "=== AyeFire Gateway Live Status ==="
    echo "ip_forward: $(cat /proc/sys/net/ipv4/ip_forward)"
    ip -br addr | grep -E "^(${WAN_IF}|${LAN_IF}(\.[0-9]+)?|wg-i1)\b" || true
    echo "--- Default Route ---"
    ip route show default || true
    echo "--- Active DHCP Leases ---"
    if [[ -f /var/lib/ayefire/dhcp.leases ]]; then
        cat /var/lib/ayefire/dhcp.leases
    else
        echo "  (no active AyeFire DHCP leases — running in staged mode)"
    fi
    echo "--- nftables ayefire_gateway ---"
    if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
        nft list table inet ayefire_gateway 2>/dev/null | head -n 45 || echo "  (table not loaded — staged mode)"
    else
        echo "  (run with sudo to inspect nftables table)"
    fi
}

case "${1:-preflight}" in
    preflight) cmd_preflight ;;
    stage)     cmd_stage ;;
    cutover)   cmd_cutover ;;
    rollback)  cmd_rollback ;;
    status)    cmd_status ;;
    *)
        echo "Usage: $0 {preflight|stage|cutover|rollback|status}" >&2
        exit 1
        ;;
esac
