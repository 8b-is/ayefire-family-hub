#![deny(clippy::float_arithmetic)]
//! AyeFire On-the-Fly Zero-Float Phoenix Audio Gateway & BLAKE3 Media Restoration Cache.
//!
//! Embeds the canonical `#![no_std]` `#![forbid(unsafe_code)]` zero-float [`PhoenixEnginePq`]
//! directly inside AyeFire so any local audio track, podcast, internet radio stream, or HTTP
//! media URL on the LAN is restored in real time block-by-block (`1024`-frame bounded pipeline,
//! unity single-stage gain — avoiding double-volume attenuation) and cached on NVMe by BLAKE3
//! content/parameter hash for instant zero-DSP, zero-WAN replay (`HTTP 206 Range` supported).

use phoenix_core::prelude::{PhoenixEnginePq, PhoenixMode, PhoenixParamsPq, PhoenixPreset};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

pub const DEFAULT_PHOENIX_CACHE_DIR: &str = ".cache/ayefire/phoenix";
/// Maximum cached restored WAV size per single finite track (256 MiB, C64 bounded).
const MAX_SINGLE_CACHE_BYTES: u64 = 256 * 1024 * 1024;
/// Streaming block size in stereo frames (1024 frames = 21.3 ms at 48 kHz, 2.66 ms at 384 kHz).
const BLOCK_FRAMES: usize = 1024;
/// Streaming RIFF header placeholder data size for infinite live radio (~2 GiB).
const STREAMING_DATA_BYTES: u32 = 0x7FFF_FFC0;

#[derive(Clone, Debug, Serialize)]
pub struct LocalAudioTrack {
    pub id: String,
    pub title: String,
    pub collection: String,
    pub ext: String,
    pub size_bytes: u64,
    #[serde(skip_serializing)]
    pub path: PathBuf,
    pub cached_presets: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhoenixLiveSnapshot {
    pub active: bool,
    pub source_label: String,
    pub preset: String,
    pub mode: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub cache_status: String,
    pub salience_left: u8,
    pub salience_right: u8,
    pub current_phi_q14: u32,
    /// Exact integer phi × 1000 (e.g., 1618 = 1.618), zero floats.
    pub phi_milli: u32,
    pub bands_q15: [i32; 5],
    pub peak_left_q15: i32,
    pub peak_right_q15: i32,
    pub frames_processed: u64,
    pub updated_unix_secs: u64,
}

impl Default for PhoenixLiveSnapshot {
    fn default() -> Self {
        Self {
            active: false,
            source_label: "Standing by — pick a track or paste any radio/audio URL".to_string(),
            preset: "natural".to_string(),
            mode: "golden_pq".to_string(),
            sample_rate: 48_000,
            bit_depth: 24,
            cache_status: "IDLE".to_string(),
            salience_left: 0,
            salience_right: 0,
            current_phi_q14: 26_510,
            phi_milli: 1618,
            bands_q15: [0; 5],
            peak_left_q15: 0,
            peak_right_q15: 0,
            frames_processed: 0,
            updated_unix_secs: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedPhoenixMeta {
    pub blake3_key: String,
    pub source_label: String,
    pub preset: String,
    pub mode: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub frames_processed: u64,
    pub wav_bytes: u64,
    pub salience_left: u8,
    pub salience_right: u8,
    pub phi_milli: u32,
    pub bands_q15: [i32; 5],
    pub created_unix_secs: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PhoenixGatewayStatus {
    pub engine: String,
    pub zero_float_enforced: bool,
    pub single_stage_unity_gain: bool,
    pub cache_dir: String,
    pub tracks_indexed: usize,
    pub cached_entries: usize,
    pub cached_bytes: u64,
    pub cached_mib: u64,
    pub streams_served: u64,
    pub active_streams: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub hit_ratio_pq: String,
    pub wan_bytes_saved: u64,
    pub frames_restored: u64,
    pub supported_rates: [u32; 4],
    pub presets: [&'static str; 8],
    pub live: PhoenixLiveSnapshot,
}

#[derive(Clone, Copy, Debug)]
pub struct StreamOptions {
    pub preset: PhoenixPreset,
    pub bypass: bool,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub allow_cache: bool,
}

impl StreamOptions {
    pub fn parse(
        preset_str: Option<&str>,
        mode_str: Option<&str>,
        rate: Option<u32>,
        bits: Option<u16>,
        allow_cache: bool,
    ) -> Self {
        let p_raw = preset_str.unwrap_or("natural").trim();
        let m_raw = mode_str.unwrap_or("").trim();
        let bypass = p_raw.eq_ignore_ascii_case("bypass")
            || p_raw.eq_ignore_ascii_case("dry")
            || p_raw.eq_ignore_ascii_case("original")
            || m_raw.eq_ignore_ascii_case("bypass")
            || m_raw.eq_ignore_ascii_case("dry");
        let preset = PhoenixPreset::from_name(p_raw).unwrap_or(PhoenixPreset::Natural);
        let sample_rate = match rate.unwrap_or(48_000) {
            44_100 => 44_100,
            48_000 => 48_000,
            96_000 => 96_000,
            192_000 => 192_000,
            384_000 => 384_000,
            r if r >= 288_000 => 384_000,
            r if r >= 144_000 => 192_000,
            r if r >= 72_000 => 96_000,
            _ => 48_000,
        };
        let bit_depth = match bits.unwrap_or(24) {
            16 => 16,
            _ => 24,
        };
        Self {
            preset,
            bypass,
            sample_rate,
            bit_depth,
            allow_cache,
        }
    }

    pub fn to_params(self) -> PhoenixParamsPq {
        let mut p = self.preset.to_params();
        if self.bypass {
            p.mode = PhoenixMode::Bypass;
        }
        // Single-stage unity gain hygiene: keep output unity at Q15_ONE unless LateNight trim
        p
    }

    pub fn preset_label(self) -> &'static str {
        if self.bypass {
            "bypass"
        } else {
            self.preset.as_str()
        }
    }

    pub fn mode_label(self) -> &'static str {
        self.to_params().mode.as_str()
    }
}

pub struct PhoenixGateway {
    pub cache_dir: PathBuf,
    pub tracks: RwLock<Vec<LocalAudioTrack>>,
    pub streams_served: AtomicU64,
    pub active_streams: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub wan_bytes_saved: AtomicU64,
    pub frames_restored: AtomicU64,
    pub live: RwLock<PhoenixLiveSnapshot>,
}

impl PhoenixGateway {
    pub fn load_from_env() -> Arc<Self> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/hue".to_string());
        let cache_dir = std::env::var("AYEFIRE_PHOENIX_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(DEFAULT_PHOENIX_CACHE_DIR));
        let _ = fs::create_dir_all(&cache_dir);

        let discovered = discover_local_audio_tracks(&home, &cache_dir);
        println!(
            "[PHOENIX-GATEWAY] zero-float PhoenixEnginePq ready | tracks={} | cache_dir={}",
            discovered.len(),
            cache_dir.display()
        );

        Arc::new(Self {
            cache_dir,
            tracks: RwLock::new(discovered),
            streams_served: AtomicU64::new(0),
            active_streams: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            wan_bytes_saved: AtomicU64::new(0),
            frames_restored: AtomicU64::new(0),
            live: RwLock::new(PhoenixLiveSnapshot::default()),
        })
    }

    pub async fn rescan_tracks(&self) -> usize {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/hue".to_string());
        let discovered = discover_local_audio_tracks(&home, &self.cache_dir);
        let n = discovered.len();
        let mut g = self.tracks.write().await;
        *g = discovered;
        n
    }

    pub async fn list_tracks(&self) -> Vec<LocalAudioTrack> {
        self.tracks.read().await.clone()
    }

    pub async fn find_track(&self, id: &str) -> Option<LocalAudioTrack> {
        self.tracks
            .read()
            .await
            .iter()
            .find(|t| t.id == id)
            .cloned()
    }

    pub async fn status(&self) -> PhoenixGatewayStatus {
        let tracks_indexed = self.tracks.read().await.len();
        let (cached_entries, cached_bytes) = scan_cache_usage(&self.cache_dir);
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let live = self.live.read().await.clone();

        PhoenixGatewayStatus {
            engine: "phoenix-core #![no_std] PhoenixEnginePq (zero-float Q31/Q15)".to_string(),
            zero_float_enforced: true,
            single_stage_unity_gain: true,
            cache_dir: self.cache_dir.display().to_string(),
            tracks_indexed,
            cached_entries,
            cached_bytes,
            cached_mib: cached_bytes / (1024 * 1024),
            streams_served: self.streams_served.load(Ordering::Relaxed),
            active_streams: self.active_streams.load(Ordering::Relaxed),
            cache_hits: hits,
            cache_misses: misses,
            hit_ratio_pq: format_reduced_pq(hits, hits.saturating_add(misses)),
            wan_bytes_saved: self.wan_bytes_saved.load(Ordering::Relaxed),
            frames_restored: self.frames_restored.load(Ordering::Relaxed),
            supported_rates: [48_000, 96_000, 192_000, 384_000],
            presets: [
                "natural",
                "vocal",
                "vintage",
                "atmos_dome",
                "intimate_club",
                "rock_stage",
                "late_night",
                "bypass",
            ],
            live,
        }
    }

    pub fn cache_key(&self, source_fingerprint: &str, opts: StreamOptions) -> String {
        let material = format!(
            "phoenix-v1|{}|{}|{}|{}|{}",
            source_fingerprint,
            opts.preset_label(),
            opts.mode_label(),
            opts.sample_rate,
            opts.bit_depth
        );
        blake3::hash(material.as_bytes()).to_hex().to_string()
    }

    pub fn cached_wav_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.wav"))
    }

    pub fn cached_meta_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.meta.json"))
    }

    /// Check if a completed BLAKE3-keyed restored WAV exists on NVMe.
    pub fn lookup_cached(&self, key: &str) -> Option<(PathBuf, Option<CachedPhoenixMeta>)> {
        let wav = self.cached_wav_path(key);
        let meta_path = self.cached_meta_path(key);
        let md = fs::metadata(&wav).ok()?;
        if md.len() < 44 {
            return None;
        }
        let meta = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str::<CachedPhoenixMeta>(&s).ok());
        Some((wav, meta))
    }

    pub async fn record_cache_hit(
        &self,
        source_label: &str,
        opts: StreamOptions,
        bytes_served: u64,
        meta: Option<&CachedPhoenixMeta>,
    ) {
        self.streams_served.fetch_add(1, Ordering::Relaxed);
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
        self.wan_bytes_saved
            .fetch_add(bytes_served, Ordering::Relaxed);
        let mut g = self.live.write().await;
        g.active = true;
        g.source_label = source_label.to_string();
        g.preset = opts.preset_label().to_string();
        g.mode = opts.mode_label().to_string();
        g.sample_rate = opts.sample_rate;
        g.bit_depth = opts.bit_depth;
        g.cache_status = "HIT (0ms DSP · NVMe)".to_string();
        if let Some(m) = meta {
            g.salience_left = m.salience_left;
            g.salience_right = m.salience_right;
            g.phi_milli = m.phi_milli;
            g.bands_q15 = m.bands_q15;
            g.frames_processed = m.frames_processed;
        }
        g.updated_unix_secs = crate::pdf_forms::now_unix_secs();
    }

    /// Spawn a bounded real-time streaming zero-float Phoenix DSP pipeline.
    ///
    /// Returns an `mpsc::Receiver<Vec<u8>>` yielding a valid streaming WAV header first,
    /// followed by restored `s24le` or `s16le` stereo PCM blocks processed through
    /// `PhoenixEnginePq<2048, 1024>`. When `cache_key` is `Some`, simultaneously tees the
    /// restored stream into NVMe cache and promotes it atomically on clean completion.
    pub fn spawn_live_stream(
        self: &Arc<Self>,
        ffmpeg_input: String,
        source_label: String,
        opts: StreamOptions,
        cache_key: Option<String>,
    ) -> anyhow::Result<mpsc::Receiver<Vec<u8>>> {
        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-i",
                &ffmpeg_input,
                "-vn",
                "-sn",
                "-dn",
                "-ac",
                "2",
                "-ar",
                &opts.sample_rate.to_string(),
                "-f",
                "s32le",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("ffmpeg stdout pipe unavailable"))?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>(16);
        let gw = Arc::clone(self);
        gw.streams_served.fetch_add(1, Ordering::Relaxed);
        gw.cache_misses.fetch_add(1, Ordering::Relaxed);
        gw.active_streams.fetch_add(1, Ordering::Relaxed);

        std::thread::spawn(move || {
            let params = opts.to_params();
            let mut engine = match PhoenixEnginePq::<2048, 1024>::new(opts.sample_rate, params) {
                Some(e) => Box::new(e),
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    gw.active_streams.fetch_sub(1, Ordering::Relaxed);
                    return;
                }
            };

            let header = build_wav_header(opts.sample_rate, 2, opts.bit_depth, STREAMING_DATA_BYTES);
            let mut tmp_file: Option<(PathBuf, fs::File, u64)> = cache_key.as_ref().and_then(|k| {
                let tmp = gw.cache_dir.join(format!(
                    "{k}.{}.tmp",
                    crate::pdf_forms::now_unix_secs()
                ));
                let mut f = fs::File::create(&tmp).ok()?;
                f.write_all(&header).ok()?;
                Some((tmp, f, 0u64))
            });

            if tx.blocking_send(header.to_vec()).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                if let Some((p, _, _)) = tmp_file.take() {
                    let _ = fs::remove_file(p);
                }
                gw.active_streams.fetch_sub(1, Ordering::Relaxed);
                return;
            }

            let raw_bytes_per_block = BLOCK_FRAMES * 8; // 2ch * 4B s32le
            let mut raw_buf = vec![0u8; raw_bytes_per_block];
            let mut in_l = [0i32; BLOCK_FRAMES];
            let mut in_r = [0i32; BLOCK_FRAMES];
            let mut out_l = [0i32; BLOCK_FRAMES];
            let mut out_r = [0i32; BLOCK_FRAMES];

            let mut block_idx: u64 = 0;
            let mut clean_eof = false;
            let mut cache_overflow = false;
            let mut client_closed = false;

            loop {
                let nread = match read_full_or_eof(&mut stdout, &mut raw_buf) {
                    Ok(0) => {
                        clean_eof = true;
                        break;
                    }
                    Ok(n) => n,
                    Err(_) => break,
                };
                let frames = nread / 8;
                if frames == 0 {
                    clean_eof = true;
                    break;
                }
                for i in 0..frames {
                    let base = i * 8;
                    in_l[i] = i32::from_le_bytes([
                        raw_buf[base],
                        raw_buf[base + 1],
                        raw_buf[base + 2],
                        raw_buf[base + 3],
                    ]);
                    in_r[i] = i32::from_le_bytes([
                        raw_buf[base + 4],
                        raw_buf[base + 5],
                        raw_buf[base + 6],
                        raw_buf[base + 7],
                    ]);
                }

                engine.process_stereo_block(
                    &in_l[..frames],
                    &in_r[..frames],
                    &mut out_l[..frames],
                    &mut out_r[..frames],
                );
                gw.frames_restored
                    .fetch_add(frames as u64, Ordering::Relaxed);

                let encoded = pack_stereo_pcm(&out_l[..frames], &out_r[..frames], opts.bit_depth);

                if let Some((_, ref mut f, ref mut written_data)) = tmp_file {
                    let next_total = written_data.saturating_add(encoded.len() as u64);
                    if next_total <= MAX_SINGLE_CACHE_BYTES {
                        if f.write_all(&encoded).is_ok() {
                            *written_data = next_total;
                        } else {
                            cache_overflow = true;
                        }
                    } else {
                        cache_overflow = true;
                    }
                }

                block_idx = block_idx.saturating_add(1);
                if block_idx == 1 || block_idx.is_multiple_of(16) {
                    let tel = engine.telemetry();
                    let phi_milli = ((tel.current_phi_q14 as u64) * 1000) >> 14;
                    if let Ok(mut g) = gw.live.try_write() {
                        g.active = true;
                        g.source_label = source_label.clone();
                        g.preset = opts.preset_label().to_string();
                        g.mode = tel.mode.as_str().to_string();
                        g.sample_rate = tel.sample_rate;
                        g.bit_depth = opts.bit_depth;
                        g.cache_status = "LIVE DSP (phoenix-core)".to_string();
                        g.salience_left = tel.salience_left;
                        g.salience_right = tel.salience_right;
                        g.current_phi_q14 = tel.current_phi_q14;
                        g.phi_milli = phi_milli as u32;
                        g.bands_q15 = tel.bands_q15;
                        g.peak_left_q15 = tel.peak_left_q15;
                        g.peak_right_q15 = tel.peak_right_q15;
                        g.frames_processed = tel.frames_processed;
                        g.updated_unix_secs = crate::pdf_forms::now_unix_secs();
                    }
                }

                if tx.blocking_send(encoded).is_err() {
                    client_closed = true;
                    break;
                }
            }

            if client_closed {
                let _ = child.kill();
            }
            let _ = child.wait();
            gw.active_streams.fetch_sub(1, Ordering::Relaxed);

            // Finalize atomic BLAKE3 cache entry on clean finite completion
            if let Some((tmp_path, mut f, data_bytes)) = tmp_file {
                if clean_eof && !cache_overflow && !client_closed && data_bytes > 0 {
                    if let Some(ref key) = cache_key {
                        let exact_data = (data_bytes.min(u32::MAX as u64 - 44)) as u32;
                        let exact_hdr =
                            build_wav_header(opts.sample_rate, 2, opts.bit_depth, exact_data);
                        if f.seek(SeekFrom::Start(0)).is_ok()
                            && f.write_all(&exact_hdr).is_ok()
                            && f.flush().is_ok()
                        {
                            drop(f);
                            let final_wav = gw.cached_wav_path(key);
                            if fs::rename(&tmp_path, &final_wav).is_ok() {
                                let tel = engine.telemetry();
                                let phi_milli = (((tel.current_phi_q14 as u64) * 1000) >> 14) as u32;
                                let meta = CachedPhoenixMeta {
                                    blake3_key: key.clone(),
                                    source_label: source_label.clone(),
                                    preset: opts.preset_label().to_string(),
                                    mode: tel.mode.as_str().to_string(),
                                    sample_rate: opts.sample_rate,
                                    bit_depth: opts.bit_depth,
                                    frames_processed: tel.frames_processed,
                                    wav_bytes: data_bytes.saturating_add(44),
                                    salience_left: tel.salience_left,
                                    salience_right: tel.salience_right,
                                    phi_milli,
                                    bands_q15: tel.bands_q15,
                                    created_unix_secs: crate::pdf_forms::now_unix_secs(),
                                };
                                if let Ok(json) = serde_json::to_string_pretty(&meta) {
                                    let _ = fs::write(gw.cached_meta_path(key), json);
                                }
                            } else {
                                let _ = fs::remove_file(&tmp_path);
                            }
                        } else {
                            drop(f);
                            let _ = fs::remove_file(&tmp_path);
                        }
                    } else {
                        drop(f);
                        let _ = fs::remove_file(&tmp_path);
                    }
                } else {
                    drop(f);
                    let _ = fs::remove_file(&tmp_path);
                }
            }
        });

        Ok(rx)
    }
}

/// Validate a remote audio/radio stream URL for on-the-fly Phoenix proxying.
pub fn validate_stream_url(raw: &str) -> Result<String, &'static str> {
    let url = raw.trim();
    if url.is_empty() || url.len() > 2048 {
        return Err("URL must be 1..2048 bytes");
    }
    if url.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return Err("URL contains control characters");
    }
    let low = url.to_ascii_lowercase();
    if !(low.starts_with("http://") || low.starts_with("https://")) {
        return Err("Only http:// and https:// stream URLs are permitted");
    }
    // Block cloud metadata SSRF endpoint
    if low.contains("169.254.169.254") {
        return Err("Link-local metadata endpoint is blocked");
    }
    Ok(url.to_string())
}

/// Build a canonical 44-byte PCM WAV header (exact integer arithmetic).
pub fn build_wav_header(
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    data_bytes: u32,
) -> [u8; 44] {
    let block_align = channels.saturating_mul(bits_per_sample / 8);
    let byte_rate = sample_rate.saturating_mul(block_align as u32);
    let riff_size = data_bytes.saturating_add(36);

    let mut hdr = [0u8; 44];
    hdr[0..4].copy_from_slice(b"RIFF");
    hdr[4..8].copy_from_slice(&riff_size.to_le_bytes());
    hdr[8..12].copy_from_slice(b"WAVE");
    hdr[12..16].copy_from_slice(b"fmt ");
    hdr[16..20].copy_from_slice(&16u32.to_le_bytes());
    hdr[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    hdr[22..24].copy_from_slice(&channels.to_le_bytes());
    hdr[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    hdr[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    hdr[32..34].copy_from_slice(&block_align.to_le_bytes());
    hdr[34..36].copy_from_slice(&bits_per_sample.to_le_bytes());
    hdr[36..40].copy_from_slice(b"data");
    hdr[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    hdr
}

/// Pack stereo Q31 samples into interleaved `s24le` (6 B/frame) or `s16le` (4 B/frame).
pub fn pack_stereo_pcm(left: &[i32], right: &[i32], bit_depth: u16) -> Vec<u8> {
    let n = left.len().min(right.len());
    if bit_depth == 16 {
        let mut out = Vec::with_capacity(n * 4);
        for i in 0..n {
            let l16 = (left[i] >> 16) as i16;
            let r16 = (right[i] >> 16) as i16;
            out.extend_from_slice(&l16.to_le_bytes());
            out.extend_from_slice(&r16.to_le_bytes());
        }
        out
    } else {
        let mut out = Vec::with_capacity(n * 6);
        for i in 0..n {
            let l24 = left[i] >> 8;
            let r24 = right[i] >> 8;
            let lb = l24.to_le_bytes();
            let rb = r24.to_le_bytes();
            out.extend_from_slice(&[lb[0], lb[1], lb[2], rb[0], rb[1], rb[2]]);
        }
        out
    }
}

/// Parse single byte-range header `bytes=start-end` into inclusive `(start, end)`.
pub fn parse_http_byte_range(header_val: &str, total_len: u64) -> Option<(u64, u64)> {
    if total_len == 0 {
        return None;
    }
    let spec = header_val.trim().strip_prefix("bytes=")?;
    let first = spec.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    if a.is_empty() {
        let suffix = b.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = total_len.saturating_sub(suffix);
        return Some((start, total_len - 1));
    }
    let start = a.parse::<u64>().ok()?;
    if start >= total_len {
        return None;
    }
    let end = if b.is_empty() {
        total_len - 1
    } else {
        b.parse::<u64>().ok()?.min(total_len - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

fn read_full_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn discover_local_audio_tracks(home: &str, cache_dir: &Path) -> Vec<LocalAudioTrack> {
    let roots = [
        ("Music", PathBuf::from(home).join("Music")),
        ("Notebook Audio", PathBuf::from(home).join("Documents/Notebook")),
        ("Downloads Audio", PathBuf::from(home).join("Downloads")),
    ];
    let allowed = ["mp3", "flac", "wav", "ogg", "opus", "m4a", "aac"];
    let mut seen_ids = HashSet::new();
    let mut tracks = Vec::new();

    for (collection, dir) in roots {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();

        for path in entries {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !allowed.contains(&ext.as_str()) {
                continue;
            }
            let Ok(md) = fs::metadata(&path) else {
                continue;
            };
            if md.len() < 1024 {
                continue;
            }
            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled Track")
                .to_string();
            let id_src = format!("{}:{}", path.display(), md.len());
            let full_hash = blake3::hash(id_src.as_bytes()).to_hex().to_string();
            let id = full_hash[..16].to_string();
            if !seen_ids.insert(id.clone()) {
                continue;
            }

            // Detect whether default natural 48k is already cached on NVMe
            let mut cached_presets = Vec::new();
            for preset_name in ["natural", "vocal", "vintage", "atmos_dome"] {
                let opts = StreamOptions::parse(Some(preset_name), None, Some(48_000), Some(24), true);
                let mat = format!(
                    "phoenix-v1|local:{}:{}|{}|{}|{}|{}",
                    id,
                    md.len(),
                    opts.preset_label(),
                    opts.mode_label(),
                    opts.sample_rate,
                    opts.bit_depth
                );
                let key = blake3::hash(mat.as_bytes()).to_hex().to_string();
                if cache_dir.join(format!("{key}.wav")).exists() {
                    cached_presets.push(preset_name.to_string());
                }
            }

            tracks.push(LocalAudioTrack {
                id,
                title,
                collection: collection.to_string(),
                ext,
                size_bytes: md.len(),
                path,
                cached_presets,
            });
            if tracks.len() >= 128 {
                return tracks;
            }
        }
    }
    tracks
}

fn scan_cache_usage(dir: &Path) -> (usize, u64) {
    let Ok(rd) = fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("wav") {
            if let Ok(md) = entry.metadata() {
                count += 1;
                bytes = bytes.saturating_add(md.len());
            }
        }
    }
    (count, bytes)
}

pub fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

pub fn format_reduced_pq(num: u64, den: u64) -> String {
    if den == 0 {
        return "0/1".to_string();
    }
    let g = gcd_u64(num, den).max(1);
    format!("{}/{}", num / g, den / g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_core::math::cordic::sin_cos_q31;

    #[test]
    fn zero_float_phoenix_block_produces_valid_24bit_wav() {
        let opts = StreamOptions::parse(Some("natural"), None, Some(48_000), Some(24), true);
        let mut engine =
            Box::new(PhoenixEnginePq::<2048, 1024>::new(opts.sample_rate, opts.to_params()).unwrap());
        let mut in_l = [0i32; 512];
        let mut in_r = [0i32; 512];
        let mut out_l = [0i32; 512];
        let mut out_r = [0i32; 512];
        for i in 0..512 {
            let ph = ((i as u64 * 1_000) << 32) / 48_000;
            let s = sin_cos_q31(ph as u32).0 >> 2;
            in_l[i] = s;
            in_r[i] = s;
        }
        engine.process_stereo_block(&in_l, &in_r, &mut out_l, &mut out_r);
        let pcm = pack_stereo_pcm(&out_l, &out_r, 24);
        assert_eq!(pcm.len(), 512 * 6);
        let hdr = build_wav_header(48_000, 2, 24, pcm.len() as u32);
        assert_eq!(&hdr[0..4], b"RIFF");
        assert_eq!(&hdr[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes([hdr[40], hdr[41], hdr[42], hdr[43]]), 3072);
        assert!(engine.telemetry().peak_left_q15 > 1000);
    }

    #[test]
    fn byte_range_and_url_validation_are_strict() {
        assert_eq!(parse_http_byte_range("bytes=0-1023", 4096), Some((0, 1023)));
        assert_eq!(parse_http_byte_range("bytes=1024-", 4096), Some((1024, 4095)));
        assert_eq!(parse_http_byte_range("bytes=-512", 4096), Some((3584, 4095)));
        assert_eq!(parse_http_byte_range("bytes=9999-", 4096), None);

        assert!(validate_stream_url("https://stream.example.org/radio.mp3").is_ok());
        assert!(validate_stream_url("file:///etc/passwd").is_err());
        assert!(validate_stream_url("http://169.254.169.254/latest/meta-data").is_err());
    }
}
