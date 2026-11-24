#![deny(clippy::float_arithmetic)]
//! Print-to-Aye IPP Virtual Printer Listener (`ipp-ngest` over RFC 8011 HTTP/IPP).
//!
//! Any phone, Mac, Windows, or Linux device on the home LAN can print straight to
//! `"AyeFire Family Printer (Fill & Help)"` — Aye receives the PDF job, runs local
//! DeepSeek `p/q` + `oxidize-pdf` form matching, prepares the filled PDF + `.eml` draft,
//! and speaks a friendly confirmation aloud (unless Incognito is active).

use crate::pdf_forms::{self, HouseholdFamilyProfile, PrintedFormJob};
use crate::sanctuary_dns::SanctuaryDnsConfig;
use crate::sanctuary_library::SanctuaryLibrary;
use ipp_ngest::model::{PageOrientation, Resolution};
use ipp_ngest::server::{serve_http, wrap_as_http_service};
use ipp_ngest::service::simple::{
    PrinterInfoBuilder, SimpleIppDocument, SimpleIppService, SimpleIppServiceHandler,
};
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io;
use tokio::sync::RwLock;
use tokio_util::compat::*;
use uuid::Uuid;

pub const DEFAULT_IPP_PORT: u16 = 6631;

#[derive(Clone)]
pub struct FamilySharedState {
    pub spool_dir: PathBuf,
    pub profile_path: PathBuf,
    pub llm_endpoint: String,
    pub llm_model: String,
    pub bundle_dir: PathBuf,
    pub ipp_port: u16,
    pub profile: Arc<RwLock<HouseholdFamilyProfile>>,
    pub forms: Arc<RwLock<VecDeque<PrintedFormJob>>>,
    pub wall_oven_working: Arc<RwLock<bool>>,
    pub ipp_jobs_received: Arc<RwLock<u64>>,
    pub library: Arc<SanctuaryLibrary>,
    pub dns: SanctuaryDnsConfig,
    pub http_port: u16,
}

struct AyeIppHandler {
    state: FamilySharedState,
}

impl SimpleIppServiceHandler for AyeIppHandler {
    fn handle_document(
        &self,
        document: SimpleIppDocument,
    ) -> impl futures::Future<Output = anyhow::Result<()>> + Send {
        let state = self.state.clone();
        async move {
            let _user = document.job_attributes.originating_user_name.clone();
            let doc_title = String::new();
            let stamp = pdf_forms::now_unix_secs();
            let raw_id = Uuid::new_v4().to_string();
            let short = &raw_id[..8];
            let pdf_path = state.spool_dir.join(format!("ipp-{stamp}-{short}.pdf"));

            tokio::fs::create_dir_all(&state.spool_dir).await?;
            let mut file = File::create(&pdf_path).await?;
            io::copy(&mut document.payload.compat(), &mut file).await?;

            {
                let mut cnt = state.ipp_jobs_received.write().await;
                *cnt = cnt.saturating_add(1);
            }

            let profile = state.profile.read().await.clone();
            let (job, extracted) = pdf_forms::process_incoming_pdf_fast(
                &pdf_path,
                &doc_title,
                "IPP_NETWORK_PRINTER",
                &state.spool_dir,
                &profile,
            )?;

            {
                let mut q = state.forms.write().await;
                if q.len() >= 32 {
                    q.pop_back();
                }
                q.push_front(job.clone());
            }

            println!(
                "[IPP] Received print job '{}' -> id={} member={} recipient={} (fast ACK)",
                job.title, job.id, job.matched_member, job.recipient_email
            );

            // Enrich with local DeepSeek p/q summary + voice prompt asynchronously so IPP ACK stays <150ms
            let bg_state = state.clone();
            let bg_job = job.clone();
            tokio::spawn(async move {
                let final_summary = match pdf_forms::summarize_form_with_deepseek(
                    &bg_job,
                    &extracted,
                    &bg_state.llm_endpoint,
                    &bg_state.llm_model,
                )
                .await
                {
                    Some(s) => {
                        let mut q = bg_state.forms.write().await;
                        if let Some(item) = q.iter_mut().find(|j| j.id == bg_job.id) {
                            item.aye_summary = s.clone();
                        }
                        s
                    }
                    None => bg_job.aye_summary.clone(),
                };
                let voice_line = format!(
                    "Aye received {} on the family printer. {}",
                    bg_job.title, final_summary
                );
                speak_if_not_incognito(&voice_line).await;
            });

            Ok(())
        }
    }
}

pub async fn spawn_ipp_listener(state: FamilySharedState) -> anyhow::Result<()> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), state.ipp_port);
    let info = PrinterInfoBuilder::default()
        .uuid(Some(Uuid::parse_str("8b15a7e0-f14e-4a7e-9c01-000000000631")?))
        .sides_supported(vec![
            "one-sided".to_string(),
            "two-sided-long-edge".to_string(),
        ])
        .printer_resolution_supported(vec![
            Resolution {
                cross_feed: 300,
                feed: 300,
                units: 3,
            },
            Resolution {
                cross_feed: 600,
                feed: 600,
                units: 3,
            },
        ])
        .printer_resolution_default(Some(Resolution {
            cross_feed: 300,
            feed: 300,
            units: 3,
        }))
        .orientation_supported(vec![PageOrientation::Portrait, PageOrientation::Landscape])
        .build()?;

    let ipp_service = Arc::new(SimpleIppService::new(
        info,
        AyeIppHandler {
            state: state.clone(),
        },
    ));

    println!(
        "[IPP] AyeFire Virtual Family Printer listening on ipp://0.0.0.0:{}/ipp/print",
        state.ipp_port
    );
    serve_http(addr, wrap_as_http_service(ipp_service)).await
}

pub async fn speak_if_not_incognito(text: &str) {
    // Check physical AyeOS Incognito status before speaking aloud
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build();
    if let Ok(c) = client {
        if let Ok(resp) = c.get("http://172.16.47.110/api/status").send().await {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                let incog = v
                    .get("household")
                    .and_then(|h| h.get("incognito_active"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                if incog {
                    println!("[VOICE] Skipped family announcement (AyeOS Incognito active)");
                    return;
                }
            }
        }
    }

    let clean = text.trim().to_string();
    if clean.is_empty() {
        return;
    }
    let _ = tokio::task::spawn_blocking(move || {
        let wav = format!("/tmp/ayefire-family-voice-{}.wav", pdf_forms::now_unix_secs());
        let synth = Command::new("/home/hue/.local/bin/kokoro-speak")
            .env("PIPER_ESPEAKNG_DATA_DIRECTORY", "/usr/share")
            .args(["-V", "af_sky", "-o", &wav, "say", &clean])
            .output();
        if let Ok(out) = synth {
            if out.status.success() {
                let _ = Command::new("pw-play").arg(&wav).status();
            }
        }
        let _ = std::fs::remove_file(&wav);
    })
    .await;
}
