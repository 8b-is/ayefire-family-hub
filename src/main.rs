#![deny(clippy::float_arithmetic)]
//! AyeFire Sovereign Family Care Hub — Main Server (`HTTP :8790` + `IPP :6631`).
//!
//! Unifies:
//!   1. Small DeepSeek `p/q` (`deepseek-r1-pqm-a161330` `492 MiB` BLAKE3-attested capsule + loopback inference)
//!   2. Alex's `zenUltraCooker` (`zen-cooker-core` Appliance Router + Pantry Zero-Waste + Honest-Time Recipes)
//!   3. Hue's Print-to-Aye IPP Listener (`ipp-ngest` `:6631` + `oxidize-pdf` Form Filler + `.eml` Drafts)

mod deepseek_pq;
mod ipp_printer;
mod pdf_forms;
mod ui;

use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use ipp_printer::FamilySharedState;
use pdf_forms::{HouseholdFamilyProfile, PrintedFormJob};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Serialize)]
struct KitchenApplianceView {
    name: String,
    working: bool,
    note: String,
}

#[derive(Serialize)]
struct KitchenInventoryView {
    zone: String,
    name: String,
    days_until_expiry: u16,
    qty: String,
}

#[derive(Serialize)]
struct KitchenEssentialView {
    name: String,
    low: bool,
    runner: String,
}

#[derive(Serialize)]
struct KitchenRecipeView {
    title: String,
    rating: String,
    visibility: String,
    honest_total_mins: u16,
    prep_mins: u16,
    cook_mins: u16,
    cleanup_mins: u16,
    school_boxes: u8,
}

#[derive(Serialize)]
struct KitchenSummaryView {
    dedication: String,
    tonight_headcount: u8,
    regular_headcount: u8,
    speakerphone_live: bool,
    wall_oven_working: bool,
    budget_summary: String,
    appliances: Vec<KitchenApplianceView>,
    inventory: Vec<KitchenInventoryView>,
    essentials: Vec<KitchenEssentialView>,
    recipes: Vec<KitchenRecipeView>,
}

fn build_kitchen_view(wall_oven_working: bool) -> KitchenSummaryView {
    let plan = zen_cooker_core::execute_family_hearth_cycle(true);
    let appliances = plan
        .appliances
        .iter()
        .map(|a| {
            let is_wall_oven = a.display_name.to_ascii_lowercase().contains("oven");
            let working = if is_wall_oven { wall_oven_working } else { a.is_working };
            let note = if is_wall_oven && wall_oven_working {
                "Online (restored by household toggle)".to_string()
            } else {
                a.status_note.clone()
            };
            KitchenApplianceView {
                name: a.display_name.clone(),
                working,
                note,
            }
        })
        .collect();

    let inventory = plan
        .inventory_used
        .iter()
        .map(|it| KitchenInventoryView {
            zone: it.zone.label().to_string(),
            name: it.name.clone(),
            days_until_expiry: it.days_until_expiry,
            qty: it.quantity_label.clone(),
        })
        .collect();

    let essentials = plan
        .household_essentials
        .iter()
        .map(|e| KitchenEssentialView {
            name: e.item_name.clone(),
            low: e.low_stock_alert,
            runner: e.assigned_runner.display_name().to_string(),
        })
        .collect();

    let recipes = plan
        .recipes_in_rotation
        .iter()
        .map(|r| KitchenRecipeView {
            title: r.title.clone(),
            rating: format!("{:.1}/5.0", r.family_rating_stars),
            visibility: r.visibility.badge().to_string(),
            honest_total_mins: r.honest_total_kitchen_mins(),
            prep_mins: r.unwashed_prep_mins,
            cook_mins: r.active_cook_mins,
            cleanup_mins: r.cleanup_and_dishes_mins,
            school_boxes: r.school_leftover_boxes,
        })
        .collect();

    KitchenSummaryView {
        dedication: plan.dedication_banner,
        tonight_headcount: plan.schedule_context.tonight_headcount,
        regular_headcount: plan.schedule_context.regular_headcount,
        speakerphone_live: plan.schedule_context.speakerphone_college_son_connected,
        wall_oven_working,
        budget_summary: format!(
            "${:.0} / ${:.0} ({})",
            plan.zip_market_feed.projected_weekly_spend_usd,
            plan.zip_market_feed.weekly_family_budget_usd,
            plan.zip_market_feed.standing()
        ),
        appliances,
        inventory,
        essentials,
        recipes,
    }
}

async fn index_handler(State(state): State<FamilySharedState>) -> Html<String> {
    Html(ui::render_family_hub_html(state.ipp_port))
}

async fn family_status_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let pq = deepseek_pq::inspect_local_pqm_bundle(
        &state.bundle_dir,
        &state.llm_endpoint,
        &state.llm_model,
    );
    let wall_oven = *state.wall_oven_working.read().await;
    let kitchen = build_kitchen_view(wall_oven);
    let forms: Vec<PrintedFormJob> = state.forms.read().await.iter().cloned().collect();
    let profile = state.profile.read().await.clone();
    let ipp_jobs = *state.ipp_jobs_received.read().await;

    Json(serde_json::json!({
        "ok": true,
        "schema": "ayefire_family_hub_v1",
        "ipp_port": state.ipp_port,
        "ipp_jobs_received": ipp_jobs,
        "deepseek_pq": pq,
        "kitchen": kitchen,
        "forms": forms,
        "profile": profile
    }))
}

async fn toggle_oven_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let mut g = state.wall_oven_working.write().await;
    *g = !*g;
    Json(serde_json::json!({ "ok": true, "wall_oven_working": *g }))
}

#[derive(Deserialize)]
struct AskPayload {
    question: String,
}

async fn ask_kitchen_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<AskPayload>,
) -> impl IntoResponse {
    let wall_oven = *state.wall_oven_working.read().await;
    let k = build_kitchen_view(wall_oven);
    let expiring: Vec<String> = k
        .inventory
        .iter()
        .map(|i| format!("{} ({}d left, {})", i.name, i.days_until_expiry, i.qty))
        .collect();
    let sys = "You are Aye, Alex & Hue's warm sovereign kitchen helper running locally on AyeFire (zenUltraCooker). Give practical, honest prep+cook+cleanup times. Strictly use ONLY the listed working cookers.";
    let working_cookers = if wall_oven {
        "Wall Oven, Dual-Zone Air Fryer Ultra, Stovetop"
    } else {
        "Dual-Zone Air Fryer Ultra (400F, 14m crisp) and Stovetop (NO wall oven available)"
    };
    let prompt = format!(
        "Working cookers right now: {}. Expiring fridge/pantry items: {}. Question for Alex: {}\nAnswer in 3 short friendly sentences with honest prep + cook + cleanup time using ONLY the working cookers.",
        working_cookers,
        expiring.join("; "),
        req.question.trim()
    );
    match deepseek_pq::generate_local_reply(&state.llm_endpoint, &state.llm_model, sys, &prompt, 180)
        .await
    {
        Ok(reply) => (StatusCode::OK, Json(serde_json::json!({ "ok": true, "reply": reply }))),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "ok": false, "error": e })),
        ),
    }
}

async fn ask_brain_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<AskPayload>,
) -> impl IntoResponse {
    let sys = "You are Aye, Hue & Alex's local sovereign family assistant running on DeepSeek p/q inside AyeFire. Keep answers warm, concise, practical, and honest.";
    match deepseek_pq::generate_local_reply(
        &state.llm_endpoint,
        &state.llm_model,
        sys,
        req.question.trim(),
        220,
    )
    .await
    {
        Ok(reply) => (StatusCode::OK, Json(serde_json::json!({ "ok": true, "reply": reply }))),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "ok": false, "error": e })),
        ),
    }
}

/// Sends a real RFC 8011 binary IPP `Print-Job` request (`application/ipp`) containing a freshly
/// generated Victor Athletic Approval Form PDF straight to our own live IPP listener on `:6631`!
async fn test_ipp_victor_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let sample_pdf = state.spool_dir.join("sample-victor-athletic-blank.pdf");
    if let Err(e) = pdf_forms::generate_sample_athletic_form_pdf(&sample_pdf) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "ok": false, "error": format!("pdf gen failed: {e}") })),
        );
    }
    let pdf_bytes = match std::fs::read(&sample_pdf) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "error": format!("pdf read failed: {e}") })),
            )
        }
    };

    let ipp_body = build_rfc8011_print_job(
        &format!("ipp://127.0.0.1:{}/ipp/print", state.ipp_port),
        "Victor Athletic Approval Form",
        "application/pdf",
        &pdf_bytes,
    );

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client
        .post(format!("http://127.0.0.1:{}/ipp/print", state.ipp_port))
        .header(header::CONTENT_TYPE, "application/ipp")
        .body(ipp_body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let latest_id = state
                .forms
                .read()
                .await
                .front()
                .map(|j| j.id.clone())
                .unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "ipp_http_status": resp.status().as_u16(),
                    "job_id": latest_id
                })),
            )
        }
        Ok(resp) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "ok": false,
                "error": format!("IPP listener returned HTTP {}", resp.status().as_u16())
            })),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "ok": false, "error": format!("IPP post error: {e}") })),
        ),
    }
}

/// Pure-Rust minimal RFC 8011 IPP/2.0 `Print-Job` (0x0002) request packet builder.
fn build_rfc8011_print_job(
    printer_uri: &str,
    job_name: &str,
    doc_format: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256 + payload.len());
    // Version 2.0, Operation Print-Job (0x0002), Request-ID = 1
    buf.extend_from_slice(&[0x02, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01]);
    // Operation attributes tag (0x01)
    buf.push(0x01);

    let push_attr = |b: &mut Vec<u8>, tag: u8, name: &str, val: &str| {
        b.push(tag);
        b.extend_from_slice(&(name.len() as u16).to_be_bytes());
        b.extend_from_slice(name.as_bytes());
        b.extend_from_slice(&(val.len() as u16).to_be_bytes());
        b.extend_from_slice(val.as_bytes());
    };
    push_attr(&mut buf, 0x47, "attributes-charset", "utf-8");
    push_attr(&mut buf, 0x48, "attributes-natural-language", "en-us");
    push_attr(&mut buf, 0x45, "printer-uri", printer_uri);
    push_attr(&mut buf, 0x42, "requesting-user-name", "hue");
    push_attr(&mut buf, 0x42, "job-name", job_name);
    push_attr(&mut buf, 0x49, "document-format", doc_format);
    // End-of-attributes tag (0x03)
    buf.push(0x03);
    buf.extend_from_slice(payload);
    buf
}

async fn upload_pdf_handler(
    State(state): State<FamilySharedState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    while let Ok(Some(field)) = multipart.next_field().await {
        let fname = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "uploaded-form.pdf".to_string());
        if let Ok(bytes) = field.bytes().await {
            if bytes.is_empty() {
                continue;
            }
            let stamp = pdf_forms::now_unix_secs();
            let path = state.spool_dir.join(format!("upload-{stamp}.pdf"));
            let _ = std::fs::create_dir_all(&state.spool_dir);
            if let Err(e) = std::fs::write(&path, &bytes) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "ok": false, "error": format!("write failed: {e}") })),
                );
            }
            let profile = state.profile.read().await.clone();
            match pdf_forms::process_incoming_pdf(
                &path,
                &fname,
                "BROWSER_UPLOAD",
                &state.spool_dir,
                &profile,
                &state.llm_endpoint,
                &state.llm_model,
            )
            .await
            {
                Ok(job) => {
                    let mut q = state.forms.write().await;
                    if q.len() >= 32 {
                        q.pop_back();
                    }
                    q.push_front(job.clone());
                    return (
                        StatusCode::OK,
                        Json(serde_json::json!({ "ok": true, "job": job })),
                    );
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "ok": false, "error": format!("process failed: {e}") })),
                    );
                }
            }
        }
    }
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "ok": false, "error": "no pdf file field uploaded" })),
    )
}

#[derive(Deserialize)]
struct UpdateFormPayload {
    #[serde(default)]
    recipient_email: Option<String>,
    #[serde(default)]
    fields: BTreeMap<String, String>,
}

async fn update_form_handler(
    State(state): State<FamilySharedState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<UpdateFormPayload>,
) -> impl IntoResponse {
    let profile = state.profile.read().await.clone();
    let mut q = state.forms.write().await;
    if let Some(job) = q.iter_mut().find(|j| j.id == id) {
        match pdf_forms::regenerate_job_artifacts(
            job,
            &req.fields,
            req.recipient_email.as_deref(),
            &profile,
        ) {
            Ok(()) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({ "ok": true, "job": job })),
                )
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "ok": false, "error": format!("{e}") })),
                )
            }
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "ok": false, "error": "form job not found" })),
    )
}

async fn download_filled_pdf_handler(
    State(state): State<FamilySharedState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let q = state.forms.read().await;
    if let Some(job) = q.iter().find(|j| j.id == id) {
        if let Ok(bytes) = std::fs::read(&job.filled_pdf_path) {
            return (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/pdf".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("inline; filename=\"{}-filled.pdf\"", job.id),
                    ),
                ],
                bytes,
            )
                .into_response();
        }
    }
    (StatusCode::NOT_FOUND, "Filled PDF not found").into_response()
}

async fn download_draft_eml_handler(
    State(state): State<FamilySharedState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let q = state.forms.read().await;
    if let Some(job) = q.iter().find(|j| j.id == id) {
        if let Ok(bytes) = std::fs::read(&job.draft_eml_path) {
            return (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "message/rfc822".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}-draft.eml\"", job.id),
                    ),
                ],
                bytes,
            )
                .into_response();
        }
    }
    (StatusCode::NOT_FOUND, "Email draft not found").into_response()
}

async fn save_profile_handler(
    State(state): State<FamilySharedState>,
    Json(profile): Json<HouseholdFamilyProfile>,
) -> impl IntoResponse {
    if let Err(e) = pdf_forms::save_profile(&state.profile_path, &profile) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "ok": false, "error": format!("{e}") })),
        );
    }
    *state.profile.write().await = profile;
    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let http_port: u16 = std::env::var("AYEFIRE_FAMILY_HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8790);
    let ipp_port: u16 = std::env::var("AYEFIRE_IPP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(ipp_printer::DEFAULT_IPP_PORT);
    let llm_endpoint = std::env::var("AYEFIRE_LLM_ENDPOINT")
        .unwrap_or_else(|_| deepseek_pq::DEFAULT_OLLAMA_ENDPOINT.to_string());
    let llm_model = std::env::var("AYEFIRE_LLM_MODEL")
        .unwrap_or_else(|_| deepseek_pq::DEFAULT_MODEL_NAME.to_string());
    let spool_dir = std::env::var("AYEFIRE_SPOOL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(pdf_forms::DEFAULT_SPOOL_DIR));
    let profile_path = std::env::var("AYEFIRE_PROFILE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(pdf_forms::DEFAULT_PROFILE_PATH));
    let bundle_dir = deepseek_pq::bundle_dir_from_env();

    std::fs::create_dir_all(&spool_dir)?;
    let profile = pdf_forms::load_or_create_profile(&profile_path);

    let pq_status = deepseek_pq::inspect_local_pqm_bundle(&bundle_dir, &llm_endpoint, &llm_model);
    println!(
        "[DEEPSEEK-PQM] bundle_present={} matrices={}/{} size={}MiB merkle={}..",
        pq_status.bundle_present,
        pq_status.ternary_matrices_found,
        pq_status.ternary_matrices_expected,
        pq_status.total_bundle_mib,
        &pq_status.merkle_root_blake3[..pq_status.merkle_root_blake3.len().min(16)]
    );

    let state = FamilySharedState {
        spool_dir,
        profile_path,
        llm_endpoint,
        llm_model,
        bundle_dir,
        ipp_port,
        profile: Arc::new(RwLock::new(profile)),
        forms: Arc::new(RwLock::new(VecDeque::with_capacity(32))),
        wall_oven_working: Arc::new(RwLock::new(false)),
        ipp_jobs_received: Arc::new(RwLock::new(0)),
    };

    // Spawn Hue's Print-to-Aye IPP listener (ipp-ngest)
    let ipp_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = ipp_printer::spawn_ipp_listener(ipp_state).await {
            eprintln!("[IPP] listener exited: {e}");
        }
    });

    // Optional mDNS AirPrint / IPP Everywhere advertisement via avahi-publish-service
    let _avahi = std::process::Command::new("avahi-publish-service")
        .args([
            "AyeFire Family Printer (Fill & Help)",
            "_ipp._tcp",
            &ipp_port.to_string(),
            "txtvers=1",
            "qtotal=1",
            "rp=ipp/print",
            "ty=AyeFire Sovereign Family PDF Assistant",
            "pdl=application/pdf,application/octet-stream",
            "URF=none",
            "Color=T",
            "Duplex=T",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/family/status", get(family_status_handler))
        .route("/api/kitchen/toggle-oven", post(toggle_oven_handler))
        .route("/api/kitchen/ask", post(ask_kitchen_handler))
        .route("/api/brain/ask", post(ask_brain_handler))
        .route("/api/forms/test-ipp-victor", post(test_ipp_victor_handler))
        .route("/api/forms/upload", post(upload_pdf_handler))
        .route("/api/forms/:id/update", post(update_form_handler))
        .route("/api/forms/:id/filled.pdf", get(download_filled_pdf_handler))
        .route("/api/forms/:id/draft.eml", get(download_draft_eml_handler))
        .route("/api/profile", post(save_profile_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], http_port));
    println!("[FAMILY-HUB] Serving AyeFire Family Care Hub on http://0.0.0.0:{http_port}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
