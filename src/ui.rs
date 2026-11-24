#![deny(clippy::float_arithmetic)]
//! AyeFire Sovereign Family Care Hub — Constellation Ink Web UI.
//!
//! Implements `modern-web-guidance` `tabs` pattern:
//! - `focusgroup="tablist nomemory"` + `focusgroupstart` with conditional `@microsoft/focusgroup-polyfill`
//! - `role="tabpanel"` with `hidden="until-found"` + `beforematch` tab sync
//! - Styling (`padding`, `border`, `background`) placed on `.panel-inner` child wrapper (not panel itself)
//! - Selected tab indicator via `[aria-selected="true"]::after` (`border-block-end: 3px solid var(--mint)`)
//! - Zero purple, zero XML, zero floating-point arithmetic.

pub fn render_family_hub_html(ipp_port: u16) -> String {
    let template = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>AyeFire Family Care Hub — DeepSeek p/q · Alex’s Kitchen · Print-to-Aye Forms</title>
<style>
:root{
  --bg:#05060a;--panel:#0d101b;--elev:#131a2b;--bdr:#243146;--bdr-hi:#36506e;
  --ink:#e9f1f7;--muted:#93a6bf;--mint:#4ecdc4;--cyan:#00f0ff;
  --ok:#2ecc71;--gold:#e8b05c;--coral:#ff5e6c;
  color-scheme:dark;accent-color:var(--mint);
}
*,*::before,*::after{box-sizing:border-box;}
body{
  margin:0;min-block-size:100dvh;background:var(--bg);color:var(--ink);
  font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;
  font-size:clamp(0.88rem,0.84rem + 0.25vw,0.98rem);line-height:1.5;
  padding-inline:clamp(14px,3vw,32px);padding-block:18px 44px;
}
.wrap{max-inline-size:1160px;margin-inline:auto;display:grid;gap:18px;}
header.top{
  display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:12px;
  background:var(--elev);border:1px solid var(--mint);border-radius:12px;padding:14px 20px;
}
.brand{display:flex;flex-wrap:wrap;align-items:center;gap:10px;}
.brand strong{color:var(--cyan);font-size:1.12rem;}
.nav-links{display:flex;flex-wrap:wrap;gap:8px;}
.pill{
  display:inline-flex;align-items:center;gap:6px;padding:4px 10px;border-radius:999px;
  border:1px solid var(--bdr-hi);background:var(--panel);font-size:0.78rem;color:var(--ink);
  text-decoration:none;
}
.pill.ok{border-color:var(--ok);color:var(--ok);}
.pill.mint{border-color:var(--mint);color:var(--mint);}
.pill.gold{border-color:var(--gold);color:var(--gold);}
.pill.cyan{border-color:var(--cyan);color:var(--cyan);}
.pill.coral{border-color:var(--coral);color:var(--coral);}

/* Accessible Single-Row Scrolling Tabs (modern-web-guidance: tabs) */
.tabs-shell{display:grid;gap:0;}
.tablist{
  display:flex;flex-wrap:nowrap;overflow-x:auto;gap:4px;
  border-block-end:1px solid var(--bdr-hi);
  scroll-snap-type:inline proximity;
  scrollbar-width:thin;
}
.tablist [role="tab"],.tablist button{
  flex:none;position:relative;scroll-snap-align:start;
  font:inherit;font-size:0.9rem;padding:12px 18px;border:0;background:transparent;
  color:var(--muted);cursor:pointer;white-space:nowrap;border-radius:8px 8px 0 0;
}
.tablist button:hover{color:var(--ink);background:var(--panel);}
.tablist button[aria-selected="true"]{color:var(--cyan);}
.tablist button[aria-selected="true"]::after{
  content:"";position:absolute;inset:auto 0 0;border-block-end:3px solid var(--mint);
}
.tablist button:focus-visible,button:focus-visible,input:focus-visible,textarea:focus-visible,select:focus-visible,a:focus-visible{
  outline:3px solid var(--cyan);outline-offset:2px;
}

/* Panels share one grid track so hidden="until-found" never leaves gap strips */
.panels-grid{display:grid;}
.panels-grid > [role="tabpanel"]{grid-area:1/1;}
/* Padding/border/background live strictly on .panel-inner (never on [role="tabpanel"]) */
.panel-inner{
  background:var(--panel);border:1px solid var(--bdr);border-block-start:0;
  border-radius:0 0 12px 12px;padding:20px;display:grid;gap:18px;
}

.grid-2{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,440px),1fr));gap:16px;}
.card{
  background:var(--elev);border:1px solid var(--bdr-hi);border-radius:10px;
  padding:16px;display:grid;gap:12px;align-content:start;
}
.card h2,.card h3{
  margin:0;color:var(--mint);font-size:1rem;padding-block-end:6px;
  border-block-end:1px solid var(--bdr);display:flex;flex-wrap:wrap;justify-content:space-between;align-items:center;gap:8px;
}
.kpi-row{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,170px),1fr));gap:10px;}
.kpi{background:var(--panel);border:1px solid var(--bdr);border-radius:8px;padding:10px 12px;display:grid;gap:2px;}
.kpi span{font-size:0.72rem;color:var(--muted);text-transform:uppercase;}
.kpi strong{font-size:1rem;color:var(--cyan);overflow-wrap:anywhere;}

.list{display:grid;gap:8px;}
.row-item{
  background:var(--panel);border:1px solid var(--bdr);border-inline-start:3px solid var(--mint);
  border-radius:8px;padding:10px 12px;display:grid;gap:4px;font-size:0.83rem;
}
.row-item.warn{border-inline-start-color:var(--gold);}
.row-item.bad{border-inline-start-color:var(--coral);}
.row-item.ok{border-inline-start-color:var(--ok);}

.btn-row{display:flex;flex-wrap:wrap;gap:8px;align-items:center;}
.btn,button.act{
  font:inherit;font-size:0.83rem;padding:8px 14px;border-radius:8px;cursor:pointer;
  background:var(--panel);color:var(--ink);border:1px solid var(--bdr-hi);text-decoration:none;
  display:inline-flex;align-items:center;gap:6px;
}
.btn.primary,button.act.primary{background:var(--mint);border-color:var(--mint);color:#05060a;font-weight:700;}
.btn:hover,button.act:hover{border-color:var(--cyan);color:var(--cyan);}
.btn.primary:hover,button.act.primary:hover{background:var(--cyan);border-color:var(--cyan);color:#05060a;}

input[type="text"],input[type="email"],textarea{
  font:inherit;font-size:0.84rem;padding:8px 10px;border-radius:6px;inline-size:100%;
  background:#060913;color:var(--ink);border:1px solid var(--bdr-hi);
}
.field-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,260px),1fr));gap:10px;}
.field{display:grid;gap:4px;}
.field label{font-size:0.78rem;color:var(--muted);font-weight:700;}
.field.needs input{border-color:var(--gold);}
.answer-box{
  background:#060913;border:1px solid var(--mint);border-radius:8px;padding:12px;
  font-size:0.85rem;white-space:pre-wrap;overflow-wrap:anywhere;min-block-size:52px;
}
.muted{color:var(--muted);} .ok{color:var(--ok);} .gold{color:var(--gold);} .cyan{color:var(--cyan);} .mint{color:var(--mint);} .coral{color:var(--coral);}
@media (prefers-reduced-motion:reduce){*,*::before,*::after{animation:none!important;transition:none!important;}}
@media (forced-colors:active){.card,.panel-inner,header.top,.pill,button,.btn{border:2px solid CanvasText;}}
</style>
</head>
<body>
<main class="wrap">
  <header class="top">
    <div class="brand">
      <strong>AyeFire Family Care Hub</strong>
      <span class="pill ok">● 100% LOCAL LAN · ZERO CLOUD</span>
      <span class="pill cyan" id="hdr-ipp">🖨️ IPP :__IPP_PORT__ READY</span>
      <span class="pill mint" id="hdr-pqm">🧠 DEEPSEEK p/q VERIFIED</span>
    </div>
    <nav class="nav-links" aria-label="AyeFire links">
      <a class="pill mint" href="/welcome">Sanctuary Front Door</a>
      <a class="pill gold" href="/?tab=phoenix">Phoenix Live Gateway</a>
      <a class="pill cyan" href="/?tab=gateway">Edge Cache &amp; Router</a>
      <a class="pill" href="http://127.0.0.1:8080/" target="_blank" rel="noopener">Phoenix Lab (:8080)</a>
      <a class="pill" href="/api/gateway/status">Gateway JSON</a>
      <a class="pill" href="/api/phoenix/status">Phoenix JSON</a>
    </nav>
  </header>

  <section class="tabs-shell">
    <div class="tablist" focusgroup="tablist nomemory" aria-label="AyeFire family care modules">
      <button type="button" id="tab-sanctuary" aria-selected="true" focusgroupstart>Sanctuary Library</button>
      <button type="button" id="tab-phoenix" aria-selected="false">Phoenix Audio (Live)</button>
      <button type="button" id="tab-gateway" aria-selected="false">Edge Cache &amp; Gateway</button>
      <button type="button" id="tab-kitchen" aria-selected="false">Alex’s Kitchen</button>
      <button type="button" id="tab-forms" aria-selected="false">Print-to-Aye (:__IPP_PORT__)</button>
      <button type="button" id="tab-brain" aria-selected="false">DeepSeek p/q Brain</button>
    </div>

    <div class="panels-grid">
      <!-- TAB 0: SANCTUARY LIBRARY (OFFLINE INTERNET OF EDUCATION, UNDERSTANDING, LOVE, RESPECT) -->
      <div role="tabpanel" id="panel-sanctuary" aria-labelledby="tab-sanctuary">
        <div class="panel-inner">
          <div class="row-item ok">
            <strong>AyeFire Sanctuary Internet — Education · Understanding · Love · Respect · Space ({&lt;3,&lt;3,&lt;3}+1)</strong>
            <span class="muted">100% offline-ready local knowledge portal. Zero cloud trackers, zero external CDNs, zero reading-history surveillance, deterministic zero-float integer search.</span>
          </div>

          <div class="kpi-row">
            <div class="kpi"><span>Indexed Manuscripts</span><strong id="lib-docs">—</strong></div>
            <div class="kpi"><span>Offline Corpus Size</span><strong class="cyan" id="lib-size">—</strong></div>
            <div class="kpi"><span>Search Terms Indexed</span><strong class="mint" id="lib-terms">—</strong></div>
            <div class="kpi"><span>Privacy &amp; Integrity</span><strong class="ok" id="lib-merkle">ZERO TRACKING ✓</strong></div>
          </div>

          <div class="card">
            <h2>
              <span>Explore Learning Paths &amp; Search Offline Library</span>
              <span class="pill ok">ZERO-FLOAT INTEGER RANKING</span>
            </h2>
            <div class="btn-row" id="lib-topic-pills"></div>
            <div class="grid-2">
              <div class="field">
                <label for="lib-query">Search papers, books, Ultralove Editions, specs, or concepts (works completely air-gapped):</label>
                <input id="lib-query" type="text" placeholder="e.g. spherepop, Six Pillars, active geodesic inference, dark energy, exact integer, cryptography">
              </div>
              <div class="field">
                <label for="lib-collection">Filter Collection:</label>
                <select id="lib-collection" style="font:inherit;font-size:0.84rem;padding:8px 10px;border-radius:6px;inline-size:100%;background:#060913;color:var(--ink);border:1px solid var(--bdr-hi);">
                  <option value="all">All Collections (Sovereign Library + Downloads + standardgalactic)</option>
                  <option value="sovereign-library">Sovereign Library (Books, Ultralove Editions, Six Pillars)</option>
                  <option value="downloads-papers">Downloads Papers (PDFs, LaTeX, Whitepapers)</option>
                  <option value="standardgalactic">Flyxion / standardgalactic Canonical Research</option>
                </select>
              </div>
            </div>
            <div class="btn-row">
              <button type="button" class="act primary" id="btn-lib-search">Search Offline Library</button>
              <button type="button" class="act" id="btn-lib-ask">Ask Local Tutor (Cited DeepSeek p/q)</button>
              <button type="button" class="act" id="btn-lib-reset">Browse Curated Shelf</button>
              <span class="muted" id="lib-status" style="font-size:0.78rem"></span>
            </div>
            <div class="answer-box" id="lib-tutor-box" style="display:none"></div>
          </div>

          <div class="grid-2">
            <article class="card">
              <h2><span>Matching Manuscripts &amp; Books</span><span class="pill cyan" id="lib-hit-count">0 shown</span></h2>
              <div class="list" id="lib-results"></div>
            </article>
            <article class="card">
              <h2><span>Offline PDF Reading Shelf (~/Downloads)</span><span class="pill gold">LOCAL VIEWER</span></h2>
              <div class="list" id="lib-pdfs"></div>
            </article>
          </div>
        </div>
      </div>

      <!-- TAB 1: ON-THE-FLY PHOENIX AUDIO GATEWAY & BLAKE3 MEDIA CACHE -->
      <div role="tabpanel" id="panel-phoenix" aria-labelledby="tab-phoenix" hidden="until-found">
        <div class="panel-inner">
          <div class="row-item ok">
            <strong>On-the-Fly Zero-Float Phoenix Audio Gateway (music.aye · Embedded #![no_std] PhoenixEnginePq)</strong>
            <span class="muted">Restores any local track, podcast, or live HTTP/HTTPS radio stream block-by-block through exact Q31/Q15 integer DSP with single-stage unity gain (zero double-volume bugs) and caches finite streams on NVMe by BLAKE3 hash for 0 ms replay.</span>
          </div>

          <div class="kpi-row">
            <div class="kpi"><span>Engine &amp; Gain Stage</span><strong class="ok" id="phx-engine">Zero-Float Unity ✓</strong></div>
            <div class="kpi"><span>Local Tracks Indexed</span><strong class="cyan" id="phx-tracks-count">—</strong></div>
            <div class="kpi"><span>NVMe Restored Cache</span><strong class="mint" id="phx-cache-kpi">—</strong></div>
            <div class="kpi"><span>Cache Hit Ratio (p/q)</span><strong class="gold" id="phx-hit-pq">0/1</strong></div>
          </div>

          <div class="card">
            <h2>
              <span>Live Streaming Player &amp; A/B Dry/Phoenix Switch</span>
              <span class="pill ok" id="phx-live-pill">IDLE</span>
            </h2>
            <div class="field-grid">
              <div class="field">
                <label for="phx-preset">Phoenix Preset:</label>
                <select id="phx-preset" style="font:inherit;font-size:0.84rem;padding:8px 10px;border-radius:6px;inline-size:100%;background:#060913;color:var(--ink);border:1px solid var(--bdr-hi);">
                  <option value="natural">Natural (GoldenPq + Marine Soul Guard)</option>
                  <option value="vocal">Vocal (P!nk / Barbra Air &amp; Presence)</option>
                  <option value="vintage">Vintage / MP3 Restoration</option>
                  <option value="atmos_dome">Atmos 3D Dome (6-Wall Wave Stage)</option>
                  <option value="intimate_club">Intimate Club (Atmos 3D)</option>
                  <option value="rock_stage">Rock Stage (Atmos 3D)</option>
                  <option value="late_night">Late Night (Warm Low-Fatigue)</option>
                </select>
              </div>
              <div class="field">
                <label for="phx-rate">Output Sample Rate:</label>
                <select id="phx-rate" style="font:inherit;font-size:0.84rem;padding:8px 10px;border-radius:6px;inline-size:100%;background:#060913;color:var(--ink);border:1px solid var(--bdr-hi);">
                  <option value="48000">48 kHz (Universal Phone / TV / Browser)</option>
                  <option value="96000">96 kHz (Studio Hi-Res)</option>
                  <option value="192000">192 kHz (Audiophile Hi-Res)</option>
                  <option value="384000">384 kHz (DAC Direct · TX-384Khz)</option>
                </select>
              </div>
              <div class="field">
                <label for="phx-bits">PCM Bit Depth:</label>
                <select id="phx-bits" style="font:inherit;font-size:0.84rem;padding:8px 10px;border-radius:6px;inline-size:100%;background:#060913;color:var(--ink);border:1px solid var(--bdr-hi);">
                  <option value="24">24-bit Lossless PCM (s24le)</option>
                  <option value="16">16-bit Universal PCM (s16le)</option>
                </select>
              </div>
            </div>

            <div class="field">
              <label for="phx-url">Or paste ANY live Internet Radio / Podcast / MP3 / FLAC URL to Phoenix on the fly:</label>
              <input id="phx-url" type="text" placeholder="https://example.org/radio-stream.mp3 or podcast episode URL">
            </div>

            <div class="btn-row">
              <button type="button" class="act primary" id="btn-phx-play-url">Stream URL Through Phoenix</button>
              <button type="button" class="act" id="btn-phx-ab">A/B Mode: PHOENIX ON (Click for Dry Bypass)</button>
              <button type="button" class="act" id="btn-phx-rescan">Rescan Local Audio</button>
              <a class="btn" href="http://127.0.0.1:8080/" target="_blank" rel="noopener">Open Full Phoenix Lab (:8080)</a>
            </div>

            <div class="row-item ok">
              <strong id="phx-now-playing">Select any local track below or paste a live stream URL</strong>
              <audio id="phx-player" controls preload="none" style="inline-size:100%;margin-top:6px"></audio>
            </div>

            <div class="kpi-row" id="phx-telemetry-row">
              <div class="kpi"><span>Marine Salience (L / R)</span><strong class="mint" id="phx-salience">0 / 0</strong></div>
              <div class="kpi"><span>Dynamic φ (milli-φ)</span><strong class="gold" id="phx-phi">1618</strong></div>
              <div class="kpi"><span>Frames Restored</span><strong class="cyan" id="phx-frames">0</strong></div>
              <div class="kpi"><span>WAN / DSP Bytes Saved</span><strong class="ok" id="phx-saved">0 KiB</strong></div>
            </div>
            <div class="row-item" id="phx-bands">5-Band MarineSense Emotional Envelopes: Warmth 0 · Presence 0 · Clarity 0 · Air 0 · Ultrasonic 0</div>
          </div>

          <article class="card">
            <h2><span>Local Audio Library (~/Music · ~/Documents/Notebook · ~/Downloads)</span><span class="pill cyan" id="phx-list-pill">0 tracks</span></h2>
            <div class="list" id="phx-track-list"></div>
          </article>
        </div>
      </div>

      <!-- TAB 2: BLAKE3 SOVEREIGN EDGE CACHE & AYEFIRE ROUTER CUTOVER -->
      <div role="tabpanel" id="panel-gateway" aria-labelledby="tab-gateway" hidden="until-found">
        <div class="panel-inner">
          <div class="row-item ok" id="gw-mode-banner">
            <strong>Inspecting live Linux kernel gateway interfaces…</strong>
            <span class="muted">Reading real /sys/class/net, /proc/net/route, /proc/net/arp, and BLAKE3 cache state.</span>
          </div>

          <div class="kpi-row">
            <div class="kpi"><span>Sovereign DNS Queries</span><strong class="cyan" id="gw-dns-q">0</strong></div>
            <div class="kpi"><span>DNS Cache / .aye Hits</span><strong class="ok" id="gw-dns-hits">0</strong></div>
            <div class="kpi"><span>Telemetry / UI Blocked</span><strong class="gold" id="gw-dns-blocked">0</strong></div>
            <div class="kpi"><span>Edge Cache Proxy</span><strong class="mint" id="gw-proxy-port">:8792</strong></div>
          </div>

          <div class="grid-2">
            <article class="card">
              <h2><span>8bsov Physical NICs &amp; Routing State</span><span class="pill ok" id="gw-fwd-pill">ip_forward</span></h2>
              <div class="list" id="gw-nics"></div>
              <h3><span>Live LAN Neighbors (/proc/net/arp)</span><span class="pill gold">REAL ARP TABLE</span></h3>
              <div class="list" id="gw-arp"></div>
            </article>

            <article class="card">
              <h2><span>BLAKE3 Sovereign Edge Cache (:8792 + Zero-MITM HTTPS)</span><span class="pill cyan" id="gw-edge-pq">0/1</span></h2>
              <div class="kpi-row">
                <div class="kpi"><span>Cached Objects</span><strong id="gw-edge-cnt">0</strong></div>
                <div class="kpi"><span>NVMe Used / Cap</span><strong class="mint" id="gw-edge-mib">0 MiB</strong></div>
                <div class="kpi"><span>WAN Saved</span><strong class="ok" id="gw-edge-saved">0 KiB</strong></div>
                <div class="kpi"><span>Zero-MITM Tunnels</span><strong class="gold" id="gw-edge-connect">0</strong></div>
              </div>
              <div class="field">
                <label for="gw-warm-url">Test BLAKE3 Edge Cache (fetch once → cached on NVMe → instant offline HIT):</label>
                <input id="gw-warm-url" type="text" placeholder="https://raw.githubusercontent.com/8b-is/ayefire-family-hub/main/Cargo.toml">
              </div>
              <div class="btn-row">
                <button type="button" class="act primary" id="btn-gw-warm">Fetch &amp; Cache via BLAKE3</button>
                <button type="button" class="act" id="btn-gw-refresh">Refresh Gateway Status</button>
                <a class="btn" href="/wpad.dat" target="_blank" rel="noopener">View /wpad.dat</a>
              </div>
              <div class="answer-box" id="gw-warm-out">Ready — LAN clients can set http_proxy=http://172.16.47.118:8792 (pacman, curl, apt, cargo) with zero OS Root CA MITM.</div>
              <div class="list" id="gw-recent-cache"></div>
            </article>
          </div>

          <article class="card">
            <h2>
              <span>Tier 3 Opt-In Hardware-Local HTTPS Inspect (Firefox Proxy :8792)</span>
              <span class="pill gold" id="insp-state-pill">CHECKING…</span>
            </h2>
            <div class="row-item ok" id="insp-summary-box">
              <strong>Hardware-Local ECDSA P-256 CA (Strictly on this machine · Never OS-wide · Instant Incognito Toggle)</strong>
              <span class="muted" id="insp-ca-meta">Loading CA fingerprint &amp; Firefox trust status…</span>
            </div>
            <div class="kpi-row">
              <div class="kpi"><span>HTTPS Inspected</span><strong class="cyan" id="insp-reqs">0</strong></div>
              <div class="kpi"><span>HTTPS Cache Hits (p/q)</span><strong class="ok" id="insp-pq">0/1</strong></div>
              <div class="kpi"><span>HTTPS Audio → Phoenix</span><strong class="gold" id="insp-phx">0</strong></div>
              <div class="kpi"><span>Trackers Blocked</span><strong class="mint" id="insp-blk">0</strong></div>
            </div>
            <div class="btn-row">
              <button type="button" class="act primary" id="btn-insp-toggle">Toggle Tier-3 HTTPS Inspect</button>
              <button type="button" class="act" id="btn-insp-phoenix">Toggle Auto-Phoenix HTTPS Audio</button>
              <button type="button" class="act" id="btn-insp-privacy">Toggle Banking/Auth Privacy Bypass</button>
              <a class="btn" href="/api/gateway/ca.crt">Download Local CA (.crt)</a>
            </div>
            <div class="list" id="insp-recent-list"></div>
          </article>

          <article class="card">
            <h2><span>Retiring the UniFi Dream Machine (Zero-Renumbering Cutover Kit)</span><span class="pill ok">INSTANT ROLLBACK</span></h2>
            <div class="row-item ok">
              <strong>1. Staged Parallel Test (Live Right Now — UDM Untouched)</strong>
              <span class="muted"><code>~/source/8b-is/ayefire-family-hub/scripts/ayefire-router/ayefire-gateway-ctl.sh stage</code> — Test `.aye` + caching DNS on `:5354`, BLAKE3 cache proxy on `:8792`, and Phoenix live audio on `:8790`.</span>
            </div>
            <div class="row-item warn">
              <strong>2. Physical Cutover (Keep Separate UniFi APs Running As-Is!)</strong>
              <span class="muted">Move ISP modem Ethernet → <code>eno2</code> (Intel 2.5G port). Leave <code>eno1</code> (10G) on your switch/APs, unplug UDM from switch, then run <code>sudo ./ayefire-gateway-ctl.sh cutover</code>. AyeFire claims exact gateway IP <code>172.16.47.67/25</code> + preserves <code>.118</code>, turns on stateful <code>nftables</code> NAT + port-53 DNS steering + DHCPv4!</span>
            </div>
            <div class="row-item">
              <strong>3. Instant Rollback Anytime</strong>
              <span class="muted"><code>sudo ./ayefire-gateway-ctl.sh rollback</code> restores <code>eno1</code> to normal DHCP client in 2 seconds.</span>
            </div>
          </article>
        </div>
      </div>

      <!-- TAB 3: ALEX'S KITCHEN (zenUltraCooker) -->
      <div role="tabpanel" id="panel-kitchen" aria-labelledby="tab-kitchen" hidden="until-found">
        <div class="panel-inner">
          <div class="kpi-row" id="kitchen-kpis">
            <div class="kpi"><span>Tonight’s Headcount</span><strong id="k-headcount">—</strong></div>
            <div class="kpi"><span>Primary Appliance</span><strong id="k-appliance" class="ok">Air Fryer Ultra ★</strong></div>
            <div class="kpi"><span>Weekly Budget Standing</span><strong id="k-budget">—</strong></div>
            <div class="kpi"><span>College Speakerphone Seat</span><strong id="k-speaker" class="mint">—</strong></div>
          </div>

          <div class="grid-2">
            <article class="card">
              <h2>
                <span>Smart Kitchen Appliance Router</span>
                <button type="button" class="act" id="btn-toggle-oven">Toggle Wall Oven Online/Broken</button>
              </h2>
              <div class="list" id="k-appliances"></div>

              <h3><span>Fridge / Freezer / Pantry Zero-Waste &amp; Essentials</span><span class="pill gold">EXPIRY SORTED</span></h3>
              <div class="list" id="k-inventory"></div>
            </article>

            <article class="card">
              <h2><span>Tonight’s Honest-Time Meals (Prep + Cook + Cleanup)</span><span class="pill ok">NO FAKE 15-MIN CLAIMS</span></h2>
              <div class="list" id="k-recipes"></div>

              <h3><span>Ask Aye (DeepSeek p/q) for Alex’s Kitchen</span><span class="pill cyan">LOCAL LOOPBACK</span></h3>
              <div class="field">
                <label for="kitchen-q">Ask about substitutions, kid lunches, or what to cook from tonight’s fridge:</label>
                <input id="kitchen-q" type="text" value="What can Alex cook in 25 minutes with the Air Fryer using what expires soonest in our fridge?">
              </div>
              <div class="btn-row">
                <button type="button" class="act primary" id="btn-ask-kitchen">Ask Aye (DeepSeek p/q)</button>
                <span class="muted" id="kitchen-ai-status" style="font-size:0.78rem"></span>
              </div>
              <div class="answer-box" id="kitchen-answer">Ready — asks your local DeepSeek p/q model grounded on tonight’s live pantry &amp; working appliances.</div>
            </article>
          </div>
        </div>
      </div>

      <!-- TAB 2: PRINT-TO-AYE IPP FORMS (VICTOR'S ATHLETIC FORMS & HOUSEHOLD PDFS) -->
      <div role="tabpanel" id="panel-forms" aria-labelledby="tab-forms" hidden="until-found">
        <div class="panel-inner">
          <div class="kpi-row">
            <div class="kpi"><span>IPP Virtual Printer URL</span><strong class="cyan" id="f-ipp-url">ipp://172.16.47.118:__IPP_PORT__/ipp/print</strong></div>
            <div class="kpi"><span>AirPrint / mDNS Service</span><strong class="ok">AyeFire Family Printer</strong></div>
            <div class="kpi"><span>IPP Print Jobs Received</span><strong id="f-ipp-count">0</strong></div>
            <div class="kpi"><span>PDF Engine</span><strong class="mint">oxidize-pdf (Pure Rust)</strong></div>
          </div>

          <div class="card">
            <h2>
              <span>Print Anything to Aye — Review, Fill &amp; Prepare Email Draft</span>
              <span class="pill gold">HUMAN SIGN-OFF REQUIRED</span>
            </h2>
            <p class="muted" style="margin:0;font-size:0.82rem">
              Print from any iPhone, Mac, Windows, or Linux device to <strong>AyeFire Family Printer</strong> (or drop a PDF below). Aye extracts fields locally, matches your on-disk Family Profile, asks local DeepSeek <code>p/q</code> for a plain-English summary, builds a filled PDF with <code>oxidize-pdf</code>, and prepares a ready-to-review <code>.eml</code> draft — never emailing or signing without your confirmation.
            </p>
            <div class="btn-row">
              <button type="button" class="act primary" id="btn-test-ipp-victor">Print Victor’s Sample Athletic Form via Real IPP</button>
              <label class="btn" for="pdf-upload-input">Upload PDF Form Manually…</label>
              <input id="pdf-upload-input" type="file" accept="application/pdf,.pdf" style="display:none">
              <button type="button" class="act" id="btn-refresh-forms">Refresh Spool Queue</button>
              <span class="muted" id="forms-status" style="font-size:0.8rem"></span>
            </div>
          </div>

          <div class="list" id="forms-list"></div>

          <article class="card">
            <h2><span>100%-Local Household Profile (Used to Pre-Fill Forms — Never Leaves Home Disk)</span><span class="pill ok">0600 LOCAL JSON</span></h2>
            <div class="field-grid" id="profile-fields"></div>
            <div class="btn-row">
              <button type="button" class="act primary" id="btn-save-profile">Save Local Family Profile</button>
              <span class="muted" id="profile-status" style="font-size:0.78rem"></span>
            </div>
          </article>
        </div>
      </div>

      <!-- TAB 3: SMALL DEEPSEEK p/q LOCAL BRAIN -->
      <div role="tabpanel" id="panel-brain" aria-labelledby="tab-brain" hidden="until-found">
        <div class="panel-inner">
          <div class="kpi-row">
            <div class="kpi"><span>AyeOS PQM Capsule</span><strong id="b-capsule">—</strong></div>
            <div class="kpi"><span>Ternary Matrices (AYEM158)</span><strong class="ok" id="b-matrices">—</strong></div>
            <div class="kpi"><span>Bundle Size on Disk</span><strong class="cyan" id="b-size">—</strong></div>
            <div class="kpi"><span>Loopback Boundary</span><strong class="mint" id="b-loopback">—</strong></div>
          </div>

          <div class="card">
            <h2><span>Verified Local DeepSeek p/q Attestation (Zero Third-Party Cloud)</span><span class="pill ok">BLAKE3 MERKLE ROOT</span></h2>
            <div class="row-item ok" id="b-proof-details">Loading local DeepSeek p/q capsule attestation…</div>
          </div>

          <div class="card">
            <h2><span>Ask Aye (Local DeepSeek p/q Family Assistant)</span><span class="pill cyan" id="b-model-pill">aye-deepseek-pqm:latest</span></h2>
            <div class="field">
              <label for="brain-q">Ask Aye anything — family schedule, form questions, homework help, or cooking ideas (100% local):</label>
              <textarea id="brain-q" rows="3" placeholder="e.g. Help me write a short polite email to Victor's coach attaching his signed athletic approval form."></textarea>
            </div>
            <div class="btn-row">
              <button type="button" class="act primary" id="btn-ask-brain">Ask Local DeepSeek p/q</button>
              <span class="muted" id="brain-status" style="font-size:0.78rem"></span>
            </div>
            <div class="answer-box" id="brain-answer">Standing by on literal loopback — nothing leaves your home network.</div>
          </div>
        </div>
      </div>
    </div>
  </section>
</main>

<script>
(function(){
  "use strict";
  // 100% air-gap safe: zero external CDN/polyfill network fetches.

  // Accessible Tabs (modern-web-guidance: tabs)
  const tablist=document.querySelector(".tablist");
  const tabs=[...tablist.querySelectorAll("button")];
  const panels=[...document.querySelectorAll('[role="tabpanel"]')];

  function selectTab(targetTab,focusIt){
    for(const t of tabs){
      const on=(t===targetTab);
      t.setAttribute("aria-selected",on?"true":"false");
      t.toggleAttribute("focusgroupstart",on);
    }
    for(const p of panels){
      const match=(p.getAttribute("aria-labelledby")===targetTab.id);
      if(match){
        p.hidden=false;
      }else{
        p.hidden="until-found";
      }
    }
    try{targetTab.scrollIntoView({block:"nearest",inline:"nearest",container:"nearest"});}catch(_e){}
    if(focusIt)targetTab.focus();
  }

  for(const t of tabs){
    t.addEventListener("click",()=>selectTab(t,false));
    t.addEventListener("keydown",(e)=>{
      const idx=tabs.indexOf(t);
      if(e.key==="ArrowRight"){e.preventDefault();selectTab(tabs[(idx+1)%tabs.length],true);}
      else if(e.key==="ArrowLeft"){e.preventDefault();selectTab(tabs[(idx+tabs.length-1)%tabs.length],true);}
      else if(e.key==="Home"){e.preventDefault();selectTab(tabs[0],true);}
      else if(e.key==="End"){e.preventDefault();selectTab(tabs[tabs.length-1],true);}
    });
  }
  for(const p of panels){
    p.addEventListener("beforematch",()=>{
      const tab=document.getElementById(p.getAttribute("aria-labelledby")||"");
      if(tab)selectTab(tab,false);
    });
  }

  let currentProfile=null;

  function esc(s){
    return String(s??"").replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;");
  }

  async function loadAll(){
    try{
      const r=await fetch("/api/family/status");
      if(!r.ok)return;
      const d=await r.json();

      // DeepSeek p/q status
      const pq=d.deepseek_pq||{};
      document.getElementById("b-capsule").textContent=pq.capsule_type||"unknown";
      document.getElementById("b-matrices").textContent=String(pq.ternary_matrices_found||0)+" / "+String(pq.ternary_matrices_expected||196)+(pq.m000_magic_valid?" (AYEM158 ✓)":"");
      document.getElementById("b-size").textContent=String(pq.total_bundle_mib||0)+" MiB";
      document.getElementById("b-loopback").textContent=pq.loopback_enforced?"ENFORCED (127.0.0.1)":"BLOCKED";
      document.getElementById("b-model-pill").textContent=pq.inference_model||"aye-deepseek-pqm:latest";
      document.getElementById("b-proof-details").innerHTML=
        "<strong>Model:</strong> "+esc(pq.model_id)+"<br>"+
        "<strong>BLAKE3 Merkle Root:</strong> <code>"+esc(pq.merkle_root_blake3)+"</code><br>"+
        "<strong>Bundle Path:</strong> <code>"+esc(pq.bundle_dir)+"</code> · <strong>Embed Magic:</strong> "+(pq.embed_magic_valid?"AYEEQ4G ✓":"missing");

      // Kitchen (zenUltraCooker)
      const k=d.kitchen||{};
      document.getElementById("k-headcount").textContent=String(k.tonight_headcount||0)+" / "+String(k.regular_headcount||0);
      document.getElementById("k-appliance").textContent=k.wall_oven_working?"Wall Oven + Air Fryer":"Air Fryer Ultra ★ (Oven Bypass)";
      document.getElementById("k-budget").textContent=k.budget_summary||"—";
      document.getElementById("k-speaker").textContent=k.speakerphone_live?"LIVE AT TABLE 🔊":"Standby";

      const appEl=document.getElementById("k-appliances");
      appEl.innerHTML=(k.appliances||[]).map(a=>
        '<div class="row-item '+(a.working?"ok":"bad")+'"><strong>['+(a.working?"ONLINE":"OFFLINE")+'] '+esc(a.name)+'</strong><span class="muted">'+esc(a.note)+'</span></div>'
      ).join("");

      const invEl=document.getElementById("k-inventory");
      const invRows=(k.inventory||[]).map(it=>
        '<div class="row-item '+(it.days_until_expiry<=2?"warn":"")+'"><strong>['+esc(it.zone)+'] '+esc(it.name)+'</strong><span class="muted">Expires in '+esc(it.days_until_expiry)+'d · '+esc(it.qty)+'</span></div>'
      );
      const essRows=(k.essentials||[]).map(es=>
        '<div class="row-item '+(es.low?"warn":"ok")+'"><strong>['+(es.low?"ADD NOW":"STOCKED")+'] '+esc(es.name)+'</strong><span class="muted">Runner: '+esc(es.runner)+'</span></div>'
      );
      invEl.innerHTML=invRows.concat(essRows).join("");

      const recEl=document.getElementById("k-recipes");
      recEl.innerHTML=(k.recipes||[]).map(rc=>
        '<div class="row-item ok"><strong>★ '+esc(rc.rating)+' · '+esc(rc.title)+' ['+esc(rc.visibility)+']</strong>'+
        '<span class="muted">Honest Total: '+esc(rc.honest_total_mins)+'m ('+esc(rc.prep_mins)+'m prep + '+esc(rc.cook_mins)+'m cook + '+esc(rc.cleanup_mins)+'m dishes) · School Lunch Boxes: +'+esc(rc.school_boxes)+'</span></div>'
      ).join("");

      // Forms & Profile
      document.getElementById("f-ipp-count").textContent=String(d.ipp_jobs_received||0);
      currentProfile=d.profile||null;
      renderProfileEditor(currentProfile);
      renderFormsList(d.forms||[]);
    }catch(_e){}
  }

  function renderProfileEditor(p){
    if(!p)return;
    const host=document.getElementById("profile-fields");
    const v=(p.members&&p.members[0])||{};
    const items=[
      ["primary_guardian","Primary Guardian",p.primary_guardian],
      ["primary_email","Primary Email",p.primary_email],
      ["primary_phone","Primary Phone",p.primary_phone],
      ["secondary_guardian","Secondary Guardian (Alex)",p.secondary_guardian],
      ["home_address","Home Address (Local Only)",p.home_address],
      ["emergency_contact_phone","Emergency Contact Phone",p.emergency_contact_phone],
      ["default_school_athletics_email","School Athletics Email",p.default_school_athletics_email],
      ["member_name","Student Name",v.name||"Victor"],
      ["member_dob","Student DOB",v.dob||""],
      ["member_school","School Name",v.school_or_org||""],
      ["member_physician","Physician Name",v.physician_name||""],
      ["member_insurance","Insurance Provider",v.insurance_provider||""]
    ];
    host.innerHTML=items.map(([k,lbl,val])=>
      '<div class="field"><label for="pf-'+k+'">'+esc(lbl)+'</label><input id="pf-'+k+'" data-pf="'+k+'" type="text" value="'+esc(val)+'"></div>'
    ).join("");
  }

  function renderFormsList(forms){
    const host=document.getElementById("forms-list");
    if(!forms.length){
      host.innerHTML='<div class="row-item"><strong>No printed forms yet.</strong><span class="muted">Click “Print Victor’s Sample Athletic Form via Real IPP” above or print from any device on your home Wi-Fi to “AyeFire Family Printer”.</span></div>';
      return;
    }
    host.innerHTML="";
    for(const j of forms){
      const card=document.createElement("article");
      card.className="card";
      const fieldHtml=(j.fields||[]).map(f=>
        '<div class="field '+(f.needs_human_confirm?"needs":"")+'">'+
        '<label>'+esc(f.label)+(f.needs_human_confirm?' <span class="gold">[confirm]</span>':' <span class="ok">[ready]</span>')+'</label>'+
        '<input type="text" data-fkey="'+esc(f.key)+'" value="'+esc(f.value)+'"></div>'
      ).join("");

      card.innerHTML=
        '<h2><span>'+esc(j.title)+' ('+esc(j.matched_member)+')</span><span class="pill '+(j.status==="CONFIRMED_READY_TO_SEND"?"ok":"gold")+'">'+esc(j.status)+' · '+esc(j.source_channel)+'</span></h2>'+
        '<div class="row-item ok"><strong>Aye (DeepSeek p/q) Summary:</strong> '+esc(j.aye_summary)+'</div>'+
        '<div class="field"><label>Email Recipient (for .eml draft)</label><input type="email" class="job-rcpt" value="'+esc(j.recipient_email)+'"></div>'+
        '<div class="field-grid">'+fieldHtml+'</div>'+
        '<div class="btn-row">'+
          '<button type="button" class="act primary btn-save-job">Confirm Fields &amp; Regenerate PDF + Email Draft</button>'+
          '<a class="btn" href="/api/forms/'+encodeURIComponent(j.id)+'/filled.pdf" target="_blank">Download Filled PDF</a>'+
          '<a class="btn" href="/api/forms/'+encodeURIComponent(j.id)+'/draft.eml">Download Email Draft (.eml)</a>'+
          '<span class="muted job-msg" style="font-size:0.78rem"></span>'+
        '</div>';

      card.querySelector(".btn-save-job").addEventListener("click",async()=>{
        const updates={};
        for(const inp of card.querySelectorAll("input[data-fkey]")){
          updates[inp.dataset.fkey]=inp.value;
        }
        const rcpt=card.querySelector(".job-rcpt").value;
        const msg=card.querySelector(".job-msg");
        msg.textContent="Regenerating PDF with oxidize-pdf…";
        const r=await fetch("/api/forms/"+encodeURIComponent(j.id)+"/update",{
          method:"POST",
          headers:{"Content-Type":"application/json"},
          body:JSON.stringify({recipient_email:rcpt,fields:updates})
        });
        const out=await r.json();
        msg.textContent=out.ok?"Updated filled PDF & .eml draft!":"Error: "+(out.error||r.status);
        if(out.ok)loadAll();
      });
      host.appendChild(card);
    }
  }

  document.getElementById("btn-toggle-oven").addEventListener("click",async()=>{
    await fetch("/api/kitchen/toggle-oven",{method:"POST"});
    loadAll();
  });

  document.getElementById("btn-ask-kitchen").addEventListener("click",async()=>{
    const q=document.getElementById("kitchen-q").value.trim();
    const st=document.getElementById("kitchen-ai-status");
    const box=document.getElementById("kitchen-answer");
    st.textContent="Asking local DeepSeek p/q…";
    const r=await fetch("/api/kitchen/ask",{
      method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({question:q})
    });
    const d=await r.json();
    box.textContent=d.reply||d.error||"No response";
    st.textContent=d.ok?"Local DeepSeek p/q replied":"Check loopback endpoint";
  });

  document.getElementById("btn-ask-brain").addEventListener("click",async()=>{
    const q=document.getElementById("brain-q").value.trim();
    if(!q)return;
    const st=document.getElementById("brain-status");
    const box=document.getElementById("brain-answer");
    st.textContent="Thinking locally on DeepSeek p/q…";
    const r=await fetch("/api/brain/ask",{
      method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({question:q})
    });
    const d=await r.json();
    box.textContent=d.reply||d.error||"No response";
    st.textContent=d.ok?"Replied locally (zero cloud)":"Check loopback endpoint";
  });

  document.getElementById("btn-test-ipp-victor").addEventListener("click",async()=>{
    const st=document.getElementById("forms-status");
    st.textContent="Generating Victor's blank Athletic PDF & sending real IPP Print-Job to :__IPP_PORT__…";
    const r=await fetch("/api/forms/test-ipp-victor",{method:"POST"});
    const d=await r.json();
    st.textContent=d.ok?"Printed via real IPP & ready for review!":"IPP test error: "+(d.error||r.status);
    loadAll();
  });

  document.getElementById("pdf-upload-input").addEventListener("change",async(ev)=>{
    const file=ev.target.files&&ev.target.files[0];
    if(!file)return;
    const st=document.getElementById("forms-status");
    st.textContent="Reading PDF with oxidize-pdf & matching local family profile…";
    const fd=new FormData();
    fd.append("file",file,file.name);
    const r=await fetch("/api/forms/upload",{method:"POST",body:fd});
    const d=await r.json();
    st.textContent=d.ok?"Form analyzed & filled draft ready!":"Upload error: "+(d.error||r.status);
    ev.target.value="";
    loadAll();
  });

  document.getElementById("btn-refresh-forms").addEventListener("click",loadAll);

  document.getElementById("btn-save-profile").addEventListener("click",async()=>{
    if(!currentProfile)return;
    const st=document.getElementById("profile-status");
    const get=(k)=>{const el=document.getElementById("pf-"+k);return el?el.value:"";};
    const next={
      ...currentProfile,
      primary_guardian:get("primary_guardian"),
      primary_email:get("primary_email"),
      primary_phone:get("primary_phone"),
      secondary_guardian:get("secondary_guardian"),
      home_address:get("home_address"),
      emergency_contact_phone:get("emergency_contact_phone"),
      default_school_athletics_email:get("default_school_athletics_email"),
      members:[{
        ...(currentProfile.members&&currentProfile.members[0]||{}),
        name:get("member_name"),
        dob:get("member_dob"),
        school_or_org:get("member_school"),
        physician_name:get("member_physician"),
        insurance_provider:get("member_insurance")
      }]
    };
    const r=await fetch("/api/profile",{
      method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(next)
    });
    const d=await r.json();
    st.textContent=d.ok?"Saved locally (mode 0600, never uploaded)":"Save error: "+(d.error||r.status);
    if(d.ok)loadAll();
  });

  // --- Sanctuary Offline Library ---
  let activeTopic="all";

  async function loadLibraryStatus(){
    try{
      const r=await fetch("/api/library/status");
      if(!r.ok)return;
      const d=await r.json();
      const lib=d.library||{};
      document.getElementById("lib-docs").textContent=String(lib.total_docs||0);
      document.getElementById("lib-size").textContent=String(lib.total_mib||0)+" MiB";
      document.getElementById("lib-terms").textContent=String(lib.total_indexed_terms||0);
      document.getElementById("lib-merkle").textContent="BLAKE3 "+String(lib.merkle_root_blake3||"").slice(0,12)+"… ✓";

      const pills=document.getElementById("lib-topic-pills");
      const allTopics=[{slug:"all",label:"All Learning Paths",count:lib.total_docs||0}].concat(lib.topics||[]);
      pills.innerHTML=allTopics.map(t=>
        '<button type="button" class="act'+(activeTopic===t.slug?" primary":"")+'" data-topic="'+esc(t.slug)+'">'+
        esc(t.label)+' ('+esc(t.count)+')</button>'
      ).join("");
      for(const btn of pills.querySelectorAll("button[data-topic]")){
        btn.addEventListener("click",()=>{
          activeTopic=btn.dataset.topic||"all";
          for(const b of pills.querySelectorAll("button[data-topic]")){
            b.classList.toggle("primary",b.dataset.topic===activeTopic);
          }
          runLibrarySearch();
        });
      }

      const pdfHost=document.getElementById("lib-pdfs");
      const pdfs=lib.pdfs||[];
      pdfHost.innerHTML=pdfs.length?pdfs.map(p=>
        '<div class="row-item ok"><strong>'+esc(p.title)+'</strong>'+
        '<span class="muted">'+esc(p.filename)+' · '+String(BigInt(p.byte_len||0)/1024n)+' KiB</span>'+
        '<div class="btn-row"><a class="btn" href="'+esc(p.url)+'" target="_blank">Open Offline PDF</a></div></div>'
      ).join(""):'<div class="row-item"><strong>No local PDFs found in ~/Downloads.</strong></div>';
    }catch(_e){}
  }

  async function runLibrarySearch(){
    const q=(document.getElementById("lib-query").value||"").trim();
    const col=document.getElementById("lib-collection").value||"all";
    const st=document.getElementById("lib-status");
    st.textContent="Searching locally…";
    try{
      const url="/api/library/search?q="+encodeURIComponent(q)+"&collection="+encodeURIComponent(col)+"&topic="+encodeURIComponent(activeTopic)+"&limit=24";
      const r=await fetch(url);
      const d=await r.json();
      const hits=d.hits||[];
      document.getElementById("lib-hit-count").textContent=String(hits.length)+" shown";
      st.textContent=q?("Found "+hits.length+" local matches (zero cloud)"):"Showing curated local shelf";
      renderLibraryHits(hits);
    }catch(_e){
      st.textContent="Local search error";
    }
  }

  function renderLibraryHits(hits){
    const host=document.getElementById("lib-results");
    if(!hits.length){
      host.innerHTML='<div class="row-item warn"><strong>No local matches for those terms.</strong><span class="muted">Try broader keywords or switch collection to All.</span></div>';
      return;
    }
    host.innerHTML=hits.map(h=>{
      const doc=h.doc||{};
      const pdfLink=doc.pdf_name?('<a class="btn" href="/api/library/pdf/'+encodeURIComponent(doc.pdf_name)+'" target="_blank">PDF</a>'):"";
      return '<div class="row-item">'+
        '<strong><a href="/read/'+encodeURIComponent(doc.id)+'" style="color:var(--cyan);text-decoration:none">'+esc(doc.title)+'</a></strong>'+
        '<span class="muted">'+esc(doc.collection)+' · '+esc(doc.repo)+' · topic: '+esc(doc.topic)+' · '+String(BigInt(doc.byte_len||0)/1024n)+' KiB'+(h.matched_terms?(' · score '+esc(h.score)):'')+'</span>'+
        '<span style="font-size:0.81rem">'+esc(h.excerpt||doc.snippet)+'</span>'+
        '<div class="btn-row"><a class="btn primary" href="/read/'+encodeURIComponent(doc.id)+'">Read Offline</a>'+pdfLink+'</div>'+
        '</div>';
    }).join("");
  }

  async function askLibraryTutor(){
    const q=(document.getElementById("lib-query").value||"").trim();
    if(!q)return;
    const col=document.getElementById("lib-collection").value||"all";
    const st=document.getElementById("lib-status");
    const box=document.getElementById("lib-tutor-box");
    box.style.display="block";
    box.textContent="Searching offline Sanctuary Library & consulting local DeepSeek p/q…";
    st.textContent="Grounded local tutor running…";
    try{
      const r=await fetch("/api/library/ask",{
        method:"POST",headers:{"Content-Type":"application/json"},
        body:JSON.stringify({question:q,collection:col,topic:activeTopic})
      });
      const d=await r.json();
      const cites=(d.citations||[]);
      let txt=(d.reply||d.error||"No reply");
      if(cites.length){
        txt+="\n\n--- Cited Local Sources ---\n"+cites.map((c,i)=>
          "["+(i+1)+"] "+(c.doc?c.doc.title:"")+" ("+(c.doc?c.doc.rel_path:"")+") -> /read/"+(c.doc?c.doc.id:"")
        ).join("\n");
      }
      box.textContent=txt;
      st.textContent=d.ai_available?"Local DeepSeek p/q answered with citations":"Offline excerpts retrieved (DeepSeek daemon resting)";
      if(cites.length)renderLibraryHits(cites);
    }catch(_e){
      box.textContent="Could not reach local Sanctuary tutor endpoint.";
      st.textContent="Local tutor error";
    }
  }

  document.getElementById("btn-lib-search").addEventListener("click",runLibrarySearch);
  document.getElementById("btn-lib-ask").addEventListener("click",askLibraryTutor);
  document.getElementById("btn-lib-reset").addEventListener("click",()=>{
    document.getElementById("lib-query").value="";
    document.getElementById("lib-collection").value="all";
    activeTopic="all";
    document.getElementById("lib-tutor-box").style.display="none";
    loadLibraryStatus().then(runLibrarySearch);
  });
  document.getElementById("lib-query").addEventListener("keydown",(e)=>{if(e.key==="Enter")runLibrarySearch();});
  document.getElementById("lib-collection").addEventListener("change",runLibrarySearch);

  // --- ON-THE-FLY PHOENIX AUDIO GATEWAY ---
  let phxBypass=false;
  let phxActiveSource=null; // {kind:"local",id,title} | {kind:"url",url}

  function fmtKiB(bytes){
    const b=Number(bytes||0);
    if(b>=1048576)return String(Math.trunc(b/1048576))+" MiB";
    if(b>=1024)return String(Math.trunc(b/1024))+" KiB";
    return String(b)+" B";
  }

  function buildPhoenixUrl(src){
    const preset=phxBypass?"bypass":document.getElementById("phx-preset").value;
    const rate=document.getElementById("phx-rate").value;
    const bits=document.getElementById("phx-bits").value;
    if(src.kind==="local"){
      return "/api/phoenix/local/"+encodeURIComponent(src.id)+"?preset="+encodeURIComponent(preset)+"&rate="+encodeURIComponent(rate)+"&bits="+encodeURIComponent(bits);
    }
    return "/api/phoenix/proxy?url="+encodeURIComponent(src.url)+"&preset="+encodeURIComponent(preset)+"&rate="+encodeURIComponent(rate)+"&bits="+encodeURIComponent(bits);
  }

  function playPhoenixSource(src){
    phxActiveSource=src;
    const player=document.getElementById("phx-player");
    const preset=phxBypass?"BYPASS (DRY)":document.getElementById("phx-preset").value.toUpperCase();
    const rate=document.getElementById("phx-rate").value;
    const bits=document.getElementById("phx-bits").value;
    const label=src.kind==="local"?src.title:src.url;
    document.getElementById("phx-now-playing").textContent="Streaming ["+preset+" · "+rate+" Hz / "+bits+"-bit]: "+label;
    player.src=buildPhoenixUrl(src);
    player.play().catch(()=>{});
    setTimeout(loadPhoenixStatus,700);
  }

  async function loadPhoenixStatus(){
    try{
      const r=await fetch("/api/phoenix/status");
      if(!r.ok)return;
      const d=await r.json();
      const p=d.phoenix||{};
      const live=p.live||{};
      document.getElementById("phx-tracks-count").textContent=String(p.tracks_indexed||0);
      document.getElementById("phx-cache-kpi").textContent=String(p.cached_entries||0)+" files ("+String(p.cached_mib||0)+" MiB)";
      document.getElementById("phx-hit-pq").textContent=p.hit_ratio_pq||"0/1";
      document.getElementById("phx-live-pill").textContent=live.cache_status||"IDLE";
      document.getElementById("phx-salience").textContent=String(live.salience_left||0)+" / "+String(live.salience_right||0);
      document.getElementById("phx-phi").textContent=String(live.phi_milli||1618)+" (×10⁻³)";
      document.getElementById("phx-frames").textContent=String(p.frames_restored||0);
      document.getElementById("phx-saved").textContent=fmtKiB(p.wan_bytes_saved||0);
      const b=live.bands_q15||[0,0,0,0,0];
      const pct=(v)=>Math.min(100,Math.trunc((Number(v||0)*100)/32768));
      document.getElementById("phx-bands").innerHTML=
        "<strong>MarineSense 5-Band Emotional Envelopes (Integer Q15):</strong> "+
        "Warmth <span class='gold'>"+pct(b[0])+"%</span> · "+
        "Presence <span class='mint'>"+pct(b[1])+"%</span> · "+
        "Clarity <span class='cyan'>"+pct(b[2])+"%</span> · "+
        "Air <span class='ok'>"+pct(b[3])+"%</span> · "+
        "Ultrasonic <span class='cyan'>"+pct(b[4])+"%</span>";
    }catch(_e){}
  }

  async function loadPhoenixTracks(){
    try{
      const r=await fetch("/api/phoenix/tracks");
      if(!r.ok)return;
      const d=await r.json();
      const tracks=d.tracks||[];
      document.getElementById("phx-list-pill").textContent=String(tracks.length)+" tracks";
      const el=document.getElementById("phx-track-list");
      if(!tracks.length){
        el.innerHTML='<div class="row-item"><strong>No local audio files found</strong><span class="muted">Drop MP3/FLAC/WAV/Opus files into ~/Music or paste a live radio URL above.</span></div>';
        return;
      }
      el.innerHTML=tracks.map(t=>{
        const badge=(t.cached_presets&&t.cached_presets.length)?'<span class="pill ok">⚡ NVMe CACHED ('+esc(t.cached_presets.join(","))+')</span>':'<span class="pill cyan">🔥 LIVE ON-THE-FLY DSP</span>';
        return '<div class="row-item ok">'+
          '<div style="display:flex;justify-content:space-between;gap:8px;flex-wrap:wrap;align-items:center"><strong>'+esc(t.title)+'</strong>'+badge+'</div>'+
          '<span class="muted">'+esc(t.collection)+' · .'+esc(t.ext)+' · '+fmtKiB(t.size_bytes)+'</span>'+
          '<div class="btn-row"><button type="button" class="act primary" data-phx-play="'+esc(t.id)+'" data-phx-title="'+esc(t.title)+'">Play Through Phoenix</button></div>'+
        '</div>';
      }).join("");
      for(const btn of el.querySelectorAll("[data-phx-play]")){
        btn.addEventListener("click",()=>playPhoenixSource({
          kind:"local",
          id:btn.getAttribute("data-phx-play"),
          title:btn.getAttribute("data-phx-title")||"Track"
        }));
      }
    }catch(_e){}
  }

  document.getElementById("btn-phx-play-url").addEventListener("click",()=>{
    const u=document.getElementById("phx-url").value.trim();
    if(!u)return;
    playPhoenixSource({kind:"url",url:u});
  });
  document.getElementById("btn-phx-ab").addEventListener("click",()=>{
    phxBypass=!phxBypass;
    document.getElementById("btn-phx-ab").textContent=phxBypass
      ?"A/B Mode: DRY BYPASS (Click for Phoenix ON)"
      :"A/B Mode: PHOENIX ON (Click for Dry Bypass)";
    if(phxActiveSource)playPhoenixSource(phxActiveSource);
  });
  for(const selId of ["phx-preset","phx-rate","phx-bits"]){
    document.getElementById(selId).addEventListener("change",()=>{
      if(phxActiveSource)playPhoenixSource(phxActiveSource);
    });
  }
  document.getElementById("btn-phx-rescan").addEventListener("click",async()=>{
    await fetch("/api/phoenix/rescan",{method:"POST"});
    loadPhoenixTracks();
    loadPhoenixStatus();
  });

  // --- EDGE CACHE & LIVE GATEWAY ---
  async function loadGatewayStatus(){
    try{
      const r=await fetch("/api/gateway/status");
      if(!r.ok)return;
      const d=await r.json();
      const hw=d.hardware||{};
      const dns=d.dns||{};
      const edge=d.edge_cache||{};
      document.getElementById("gw-mode-banner").innerHTML=
        '<strong>Gateway Mode: '+esc(hw.mode||"unknown")+'</strong>'+
        '<span class="muted">Default route: '+esc(hw.default_gateway_ip)+' via '+esc(hw.default_gateway_iface)+' · DNS mode: '+esc(dns.mode)+' (upstream '+esc(dns.upstream_primary)+', WAN DNS '+(dns.wan_dns_reachable?'UP ✓':'OFFLINE FAILOVER')+')</span>';
      document.getElementById("gw-fwd-pill").textContent=hw.ip_forward_enabled?"ip_forward=1 ✓":"ip_forward=0 (STAGED)";
      document.getElementById("gw-dns-q").textContent=String(dns.queries_answered||0);
      document.getElementById("gw-dns-hits").textContent=String(dns.cache_hits||0)+" cache / "+String(dns.aye_local_hits||0)+" .aye";
      document.getElementById("gw-dns-blocked").textContent=String(dns.sinkhole_blocked||0);
      document.getElementById("gw-proxy-port").textContent=":"+String(edge.proxy_port||8792)+" (Zero-MITM)";
      document.getElementById("gw-edge-pq").textContent="Hit "+String(edge.hit_ratio_pq||"0/1");
      document.getElementById("gw-edge-cnt").textContent=String(edge.entries||0);
      document.getElementById("gw-edge-mib").textContent=String(edge.total_mib||0)+" / "+String(edge.max_mib||0)+" MiB";
      document.getElementById("gw-edge-saved").textContent=fmtKiB(edge.wan_bytes_saved||0);
      document.getElementById("gw-edge-connect").textContent=String(edge.connect_tunnels||0);

      document.getElementById("gw-nics").innerHTML=(hw.interfaces||[]).map(n=>{
        const cls=n.carrier?"ok":(n.name==="eno2"?"warn":"");
        const spd=n.speed_mbps>0?(String(n.speed_mbps)+" Mbps"):"no link";
        return '<div class="row-item '+cls+'"><strong>'+esc(n.name)+' · '+esc(n.operstate.toUpperCase())+' ('+esc(spd)+')</strong>'+
          '<span class="muted">'+esc(n.role)+' · MAC '+esc(n.mac)+' · RX '+String(n.rx_mib)+' MiB / TX '+String(n.tx_mib)+' MiB</span></div>';
      }).join("");

      const arp=hw.lan_neighbors||[];
      document.getElementById("gw-arp").innerHTML=arp.length?arp.map(a=>
        '<div class="row-item '+(a.is_current_gateway?"warn":"ok")+'"><strong>'+esc(a.ip)+(a.is_current_gateway?' [CURRENT GATEWAY]':'')+' · '+esc(a.vendor_hint)+'</strong>'+
        '<span class="muted">MAC '+esc(a.mac)+' on '+esc(a.device)+'</span></div>'
      ).join(""):'<div class="row-item"><strong>No ARP neighbors visible yet</strong></div>';

      const rec=edge.recent||[];
      document.getElementById("gw-recent-cache").innerHTML=rec.slice(0,8).map(ev=>
        '<div class="row-item '+(ev.status==="HIT"?"ok":"")+'"><strong>['+esc(ev.status)+'] '+esc(ev.host)+' · '+fmtKiB(ev.size_bytes)+'</strong>'+
        '<span class="muted">BLAKE3 '+esc(ev.blake3_short)+'… · '+esc(ev.content_type)+' · '+esc(ev.url_preview)+'</span></div>'
      ).join("");

      renderInspectStatus(d.inspect||{});
    }catch(_e){}
  }

  let currentInspect={inspect_enabled:false,auto_phoenix_audio:true,privacy_bypass_enabled:true};
  function renderInspectStatus(insp){
    currentInspect=insp;
    const pill=document.getElementById("insp-state-pill");
    pill.textContent=insp.inspect_enabled?"TIER 3 ACTIVE (LOCAL CA)":"TIER 1 ZERO-MITM (OFF)";
    pill.className="pill "+(insp.inspect_enabled?"ok":"gold");
    document.getElementById("insp-ca-meta").textContent=
      (insp.ca_common_name||"Local CA")+" · BLAKE3 "+(insp.ca_blake3_fingerprint||"").slice(0,16)+"… · Firefox cert9.db: "+(insp.firefox_cert_installed?"TRUSTED ✓":"NOT IMPORTED YET")+" · Leaf certs cached: "+String(insp.leaf_certs_cached||0)+" · Privacy bypasses: "+String(insp.https_privacy_bypassed||0);
    document.getElementById("insp-reqs").textContent=String(insp.https_requests_inspected||0)+" ("+String(insp.https_tunnels_inspected||0)+" TLS)";
    document.getElementById("insp-pq").textContent=String(insp.https_cache_hits||0)+" ("+String(insp.https_hit_ratio_pq||"0/1")+")";
    document.getElementById("insp-phx").textContent=String(insp.https_phoenix_restored||0);
    document.getElementById("insp-blk").textContent=String(insp.connect_blocked||0);
    document.getElementById("btn-insp-toggle").textContent=insp.inspect_enabled
      ?"Tier-3 Inspect: ON (Click for Incognito / Zero-MITM)"
      :"Tier-3 Inspect: OFF (Click to Enable for Firefox)";
    document.getElementById("btn-insp-phoenix").textContent="Auto-Phoenix HTTPS Audio: "+(insp.auto_phoenix_audio?"ON ✓":"OFF");
    document.getElementById("btn-insp-privacy").textContent="Banking/Auth Bypass: "+(insp.privacy_bypass_enabled?"ON (Protected)":"OFF (Inspect All)");
    const evs=insp.recent||[];
    document.getElementById("insp-recent-list").innerHTML=evs.slice(0,10).map(ev=>{
      const cls=(ev.action==="HTTPS-CACHE-HIT"||ev.action==="PHOENIX-LIVE"||ev.action==="PHOENIX-HIT")?"ok":(ev.action==="SINKHOLE-BLOCKED"?"warn":"");
      return '<div class="row-item '+cls+'"><strong>['+esc(ev.action)+'] '+esc(ev.method)+' '+esc(ev.host)+esc(ev.path_preview)+' ('+String(ev.status)+')</strong>'+
        '<span class="muted">'+esc(ev.content_type)+' · '+fmtKiB(ev.size_bytes)+'</span></div>';
    }).join("");
  }

  async function patchInspect(patch){
    try{
      const r=await fetch("/api/gateway/inspect",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(patch)});
      if(!r.ok)return;
      const d=await r.json();
      if(d.inspect)renderInspectStatus(d.inspect);
    }catch(_e){}
  }
  document.getElementById("btn-insp-toggle").addEventListener("click",()=>patchInspect({inspect_enabled:!currentInspect.inspect_enabled}));
  document.getElementById("btn-insp-phoenix").addEventListener("click",()=>patchInspect({auto_phoenix_audio:!currentInspect.auto_phoenix_audio}));
  document.getElementById("btn-insp-privacy").addEventListener("click",()=>patchInspect({privacy_bypass_enabled:!currentInspect.privacy_bypass_enabled}));

  document.getElementById("btn-gw-warm").addEventListener("click",async()=>{
    const url=document.getElementById("gw-warm-url").value.trim();
    if(!url)return;
    const out=document.getElementById("gw-warm-out");
    out.textContent="Fetching through AyeFire BLAKE3 Edge Cache…";
    try{
      const r=await fetch("/api/cache/warm",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({url})});
      const d=await r.json();
      if(d.ok){
        out.textContent="["+d.cache_status+"] HTTP "+d.http_status+" · "+fmtKiB(d.size_bytes)+" · "+d.content_type+"\nBLAKE3 content hash: "+d.content_blake3+"\nClick again to verify an instant 0-WAN-byte NVMe cache HIT!";
        loadGatewayStatus();
      }else{
        out.textContent="Cache error: "+(d.error||"failed");
      }
    }catch(e){out.textContent="Request failed: "+e;}
  });
  document.getElementById("btn-gw-refresh").addEventListener("click",loadGatewayStatus);

  // Honor ?tab=sanctuary|phoenix|gateway|kitchen|forms|brain from Host routing & /welcome cards
  const wantedTab=new URLSearchParams(window.location.search).get("tab");
  if(wantedTab){
    const target=document.getElementById("tab-"+wantedTab);
    if(target)selectTab(target,false);
  }

  loadAll();
  loadLibraryStatus().then(runLibrarySearch);
  loadPhoenixStatus();
  loadPhoenixTracks();
  loadGatewayStatus();
  setInterval(loadPhoenixStatus,4000);
})();
</script>
</body>
</html>
"##;

    template.replace("__IPP_PORT__", &ipp_port.to_string())
}

/// Calm, welcoming offline front door for `http://home.aye/welcome` & air-gap captive portal landings.
/// Zero purple, zero XML, zero external CDNs, zero trackers.
pub fn render_welcome_html(
    st: &crate::sanctuary_library::SanctuaryStatus,
    http_port: u16,
    ipp_port: u16,
    dns_port: u16,
    lan_ip: &str,
) -> String {
    let port_suffix = if http_port == 80 {
        String::new()
    } else {
        format!(":{http_port}")
    };
    let merkle_short = &st.merkle_root_blake3[..st.merkle_root_blake3.len().min(16)];
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>Welcome to AyeFire Sanctuary Internet — Education · Understanding · Love · Respect</title>
<style>
:root{{
  --bg:#05060a;--panel:#0d101b;--elev:#131a2b;--bdr:#243146;--bdr-hi:#36506e;
  --ink:#e9f1f7;--muted:#93a6bf;--mint:#4ecdc4;--cyan:#00f0ff;--ok:#2ecc71;--gold:#e8b05c;
  color-scheme:dark;
}}
*{{box-sizing:border-box;}}
body{{
  margin:0;min-block-size:100dvh;background:var(--bg);color:var(--ink);
  font-family:ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,sans-serif;
  line-height:1.55;padding:24px clamp(16px,4vw,40px) 56px;
}}
.wrap{{max-inline-size:1040px;margin-inline:auto;display:grid;gap:20px;}}
header.hero{{
  background:var(--elev);border:1px solid var(--mint);border-radius:14px;
  padding:24px 26px;display:grid;gap:10px;
}}
.kicker{{font-size:0.82rem;letter-spacing:0.08em;text-transform:uppercase;color:var(--mint);font-weight:700;}}
h1{{margin:0;font-size:clamp(1.4rem,1.2rem + 1.2vw,2rem);color:var(--cyan);line-height:1.25;}}
.lead{{margin:0;color:var(--ink);max-inline-size:74ch;font-size:0.98rem;}}
.pills{{display:flex;flex-wrap:wrap;gap:8px;padding-top:4px;}}
.pill{{
  display:inline-flex;align-items:center;padding:4px 11px;border-radius:999px;
  border:1px solid var(--bdr-hi);background:var(--panel);font-size:0.78rem;color:var(--ink);
}}
.pill.ok{{border-color:var(--ok);color:var(--ok);}}
.pill.mint{{border-color:var(--mint);color:var(--mint);}}
.pill.gold{{border-color:var(--gold);color:var(--gold);}}
.doors{{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,290px),1fr));gap:16px;}}
a.door{{
  text-decoration:none;color:var(--ink);background:var(--panel);
  border:1px solid var(--bdr-hi);border-top:3px solid var(--mint);border-radius:12px;
  padding:18px;display:grid;gap:8px;align-content:start;
}}
a.door:hover{{border-color:var(--cyan);background:var(--elev);}}
a.door.gold{{border-top-color:var(--gold);}}
a.door.ok{{border-top-color:var(--ok);}}
.door strong{{font-size:1.05rem;color:var(--cyan);}}
.door code{{font-size:0.8rem;color:var(--mint);}}
.door p{{margin:0;font-size:0.86rem;color:var(--muted);}}
section.pledge{{
  background:var(--panel);border:1px solid var(--bdr);border-radius:12px;
  padding:18px 22px;display:grid;gap:8px;
}}
section.pledge h2{{margin:0;font-size:1rem;color:var(--gold);}}
section.pledge p{{margin:0;font-size:0.88rem;color:var(--muted);}}
footer{{text-align:center;font-size:0.8rem;color:var(--muted);}}
</style>
</head>
<body>
<main class="wrap">
  <header class="hero">
    <div class="kicker">AyeFire Sovereign Sanctuary · Local Offline Internet</div>
    <h1>Welcome Home — Education, Understanding, Love &amp; Respect</h1>
    <p class="lead">
      You are connected to a quiet, self-sufficient local internet that works even when unplugged from the outside world.
      No ads, no algorithmic outrage, no surveillance, and no reading-history logs — just books, research, music, a local AI tutor, and family tools.
    </p>
    <div class="pills">
      <span class="pill ok">● 100% OFFLINE READY</span>
      <span class="pill mint">{docs} Indexed Manuscripts ({mib} MiB)</span>
      <span class="pill gold">{pdfs} Offline PDFs</span>
      <span class="pill">Sovereign .aye DNS :{dns_port} → {lan_ip}</span>
      <span class="pill ok">BLAKE3 {merkle_short}…</span>
    </div>
  </header>

  <section class="doors" aria-label="Sanctuary doors">
    <a class="door" href="/?tab=sanctuary">
      <strong>Sanctuary Library &amp; Learning Paths</strong>
      <code>library.aye · learn.aye</code>
      <p>Search {docs} local books, Ultralove Editions, Flyxion research papers, and PDFs with zero-float deterministic ranking.</p>
    </a>
    <a class="door ok" href="/?tab=sanctuary">
      <strong>Ask Local Tutor (DeepSeek p/q)</strong>
      <code>ask.aye</code>
      <p>Loopback sovereign AI tutor grounded strictly in cited local passages — honest provenance, zero cloud egress.</p>
    </a>
    <a class="door gold" href="/?tab=phoenix">
      <strong>On-the-Fly Phoenix Audio Gateway</strong>
      <code>music.aye · Zero-Float PhoenixEnginePq</code>
      <p>Stream local tracks or live radio URLs through real-time zero-float Phoenix DSP with BLAKE3 NVMe caching up to 384 kHz.</p>
    </a>
    <a class="door" href="/?tab=gateway">
      <strong>Edge Cache &amp; Live Gateway</strong>
      <code>router.aye · cache.aye · Proxy :8792</code>
      <p>BLAKE3 LAN pull-through cache, telemetry-blocking DNS, live 10G/2.5G NIC status, and UDM cutover control.</p>
    </a>
    <a class="door" href="/?tab=kitchen">
      <strong>Alex’s Kitchen (zenUltraCooker)</strong>
      <code>kitchen.aye</code>
      <p>Honest-time family meals, zero-waste pantry tracker, and working-appliance routing.</p>
    </a>
    <a class="door ok" href="/?tab=forms">
      <strong>Print-to-Aye Family Assistant</strong>
      <code>print.aye · IPP :{ipp_port}</code>
      <p>Print any PDF from a phone or laptop on the LAN to fill forms locally with human sign-off.</p>
    </a>
    <a class="door gold" href="/api/sanctuary/manifest">
      <strong>Public Commons Mirror API</strong>
      <code>8a.is · 8b.is · tai.is</code>
      <p>Compact JSON manifest &amp; JSONL corpus stream for mirroring this verified library to 8a.is, 8b.is, and tai.is.</p>
    </a>
  </section>

  <section class="pledge">
    <h2>The Six Pillars of Holding</h2>
    <p>
      <strong>Compassion · Understanding · Moderation · Love · Respect · Space.</strong>
      Because love without space becomes control, understanding without respect becomes judgment, and curiosity belongs to everyone who enters the room.
      Local endpoints: <code>http://home.aye{port_suffix}/welcome</code> · <code>http://library.aye{port_suffix}/</code> · <code>http://music.aye{port_suffix}/</code>.
    </p>
  </section>

  <footer>AyeFire Sanctuary Internet · 8b-is Family · &#123;&lt;3,&lt;3,&lt;3&#125;+1</footer>
</main>
</body>
</html>"##,
        docs = st.total_docs,
        mib = st.total_mib,
        pdfs = st.pdfs.len(),
        dns_port = dns_port,
        lan_ip = lan_ip,
        ipp_port = ipp_port,
        port_suffix = port_suffix,
        merkle_short = merkle_short,
    )
}
