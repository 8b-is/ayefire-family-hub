#!/usr/bin/env bash
# AyeFire Sovereign Gateway Cutover Controller (UDM -> AyeFire on 8bsov)
#
# Safe, idempotent, zero-surprise controller with 4 explicit actions:
#   preflight  - Read-only check of eno1 (10G LAN), eno2 (2.5G WAN), wg-i1, services
#   stage      - Keep UDM live; verify staged DNS (:5354), Proxy (:8792), Phoenix (:8790)
#   cutover    - Take over WAN on eno2 + LAN Gateway 172.16.47.67/25 on eno1 (retire UDM!)
#   rollback   - Restore eno1 back to normal DHCP client (if plugging UDM back in)
#   status     - Show live NAT, DHCP leases, interface carriers, and default route

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
WAN_IF="${AYEFIRE_WAN_IF:-eno2}"
LAN_IF="${AYEFIRE_LAN_IF:-eno1}"
LAN_GW_CIDR="${AYEFIRE_LAN_GW_CIDR:-172.16.47.67/25}"
LAN_ALIAS_CIDR="${AYEFIRE_LAN_ALIAS_CIDR:-172.16.47.118/25}"
NFT_CONF="$SCRIPT_DIR/ayefire-nftables.conf"
DHCP_CONF="$SCRIPT_DIR/ayefire-dnsmasq-dhcp.conf"
DHCP_PID="/run/ayefire-dnsmasq.pid"

need_root() {
    if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
        echo "[ERROR] '$1' requires root privileges. Run: sudo $0 $1" >&2
        exit 1
    fi
}

cmd_preflight() {
    echo "=== AyeFire Gateway Preflight Check ==="
    for iface in "$LAN_IF" "$WAN_IF" wlan0 wg-i1; do
        if [[ -d "/sys/class/net/$iface" ]]; then
            state="$(cat "/sys/class/net/$iface/operstate" 2>/dev/null || echo unknown)"
            carrier="$(cat "/sys/class/net/$iface/carrier" 2>/dev/null || echo 0)"
            speed="$(cat "/sys/class/net/$iface/speed" 2>/dev/null || echo -1)"
            printf "  %-6s  state=%-8s carrier=%s  speed=%s Mbps\n" "$iface" "$state" "$carrier" "$speed"
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
    echo "Staged endpoints you can point any LAN device at RIGHT NOW (zero UDM changes):"
    echo "  • AyeFire Hub / Sanctuary / Phoenix : http://172.16.47.118:8790/"
    echo "  • Sovereign .aye + Caching DNS      : 172.16.47.118 (UDP port 5354)"
    echo "  • BLAKE3 LAN HTTP Cache Proxy       : http://172.16.47.118:8792"
    echo "  • Print-to-Aye IPP Printer          : ipp://172.16.47.118:6631/ipp/print"
    echo ""
    echo "When you are ready to physically unplug the UniFi Dream Machine:"
    echo "  1. Move ISP modem Ethernet cable -> 8bsov '$WAN_IF' (Intel 2.5G port)."
    echo "  2. Leave 8bsov '$LAN_IF' (10G port) plugged into your LAN switch / UniFi APs."
    echo "  3. Run: sudo $0 cutover"
}

cmd_cutover() {
    need_root cutover
    echo "=== AyeFire Live Gateway Cutover (Retiring UDM) ==="

    # 1. Verify WAN interface has physical link before breaking existing UDM route!
    ip link set "$WAN_IF" up
    sleep 1
    carrier="$(cat "/sys/class/net/$WAN_IF/carrier" 2>/dev/null || echo 0)"
    if [[ "$carrier" != "1" ]]; then
        echo "[ABORT] No physical Ethernet link detected on WAN interface '$WAN_IF' (carrier=$carrier)!" >&2
        echo "        Plug your ISP modem into '$WAN_IF' (Intel 2.5G port) first, then re-run cutover." >&2
        exit 2
    fi

    # 2. Bring up WAN DHCP via NetworkManager on eno2
    echo "[1/6] Requesting ISP WAN lease on $WAN_IF..."
    if nmcli -t -f NAME con show | grep -qx "ayefire-wan"; then
        nmcli con up ayefire-wan
    else
        nmcli con add type ethernet ifname "$WAN_IF" con-name ayefire-wan ipv4.method auto ipv6.method auto
        nmcli con up ayefire-wan
    fi

    # 3. Assign exact UDM gateway IP (172.16.47.67/25) + preserve .118 alias on eno1
    echo "[2/6] Configuring $LAN_IF as LAN Gateway $LAN_GW_CIDR (preserving $LAN_ALIAS_CIDR)..."
    if nmcli -t -f NAME con show | grep -qx "ayefire-lan"; then
        nmcli con mod ayefire-lan ipv4.method manual ipv4.addresses "$LAN_GW_CIDR,$LAN_ALIAS_CIDR" ipv4.never-default yes
        nmcli con up ayefire-lan
    else
        nmcli con add type ethernet ifname "$LAN_IF" con-name ayefire-lan \
            ipv4.method manual ipv4.addresses "$LAN_GW_CIDR,$LAN_ALIAS_CIDR" ipv4.never-default yes
        nmcli con up ayefire-lan
    fi

    # 4. Enable kernel IPv4/IPv6 packet forwarding
    echo "[3/6] Enabling Linux kernel IP forwarding..."
    sysctl -w net.ipv4.ip_forward=1 >/dev/null
    sysctl -w net.ipv6.conf.all.forwarding=1 >/dev/null
    cat >/etc/sysctl.d/90-ayefire-gateway.conf <<'SYSCTL'
net.ipv4.ip_forward = 1
net.ipv6.conf.all.forwarding = 1
SYSCTL

    # 5. Load stateful nftables firewall + NAT + DNS port-53 redirect
    echo "[4/6] Loading stateful AyeFire nftables firewall & NAT ($NFT_CONF)..."
    nft -f "$NFT_CONF"

    # 6. Start DHCPv4 server on eno1
    echo "[5/6] Starting AyeFire DHCPv4 server on $LAN_IF ($DHCP_CONF)..."
    mkdir -p /var/lib/ayefire
    if [[ -f "$DHCP_PID" ]]; then
        kill "$(cat "$DHCP_PID")" 2>/dev/null || true
        rm -f "$DHCP_PID"
    fi
    dnsmasq -C "$DHCP_CONF" --pid-file="$DHCP_PID"

    echo "[6/6] Live verification:"
    ip -br addr show "$WAN_IF" "$LAN_IF"
    ip route show default
    echo "[DONE] AyeFire is now your live edge router on $LAN_GW_CIDR! Your UniFi Dream Machine can stay unplugged."
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
    nmcli con down ayefire-wan 2>/dev/null || true
    nmcli con down ayefire-lan 2>/dev/null || true
    nmcli con up "Wired connection 1" || nmcli dev connect "$LAN_IF"
    echo "[DONE] Restored $LAN_IF to standard DHCP client. Plug the UDM back in if needed."
}

cmd_status() {
    echo "=== AyeFire Gateway Live Status ==="
    echo "ip_forward: $(cat /proc/sys/net/ipv4/ip_forward)"
    ip -br addr show "$LAN_IF" "$WAN_IF" wg-i1 2>/dev/null || true
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
        nft list table inet ayefire_gateway 2>/dev/null | head -n 35 || echo "  (table not loaded — staged mode)"
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
