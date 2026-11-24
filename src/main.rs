#![deny(clippy::float_arithmetic)]
//! AyeFire Sovereign Family Care Hub — Main Server (`HTTP :8790` + `IPP :6631`).
//!
//! Unifies:
//!   1. Small DeepSeek `p/q` (`deepseek-r1-pqm-a161330` `492 MiB` BLAKE3-attested capsule + loopback inference)
//!   2. Alex's `zenUltraCooker` (`zen-cooker-core` Appliance Router + Pantry Zero-Waste + Honest-Time Recipes)
//!   3. Hue's Print-to-Aye IPP Listener (`ipp-ngest` `:6631` + `oxidize-pdf` Form Filler + `.eml` Drafts)

mod deepseek_pq;
mod edge_cache;
mod gateway_status;
mod id1_transparency;
mod ipp_printer;
mod pdf_forms;
mod phoenix_gateway;
mod sanctuary_dns;
mod sanctuary_library;
mod tls_inspect;
mod transparency_radar;
mod ui;

use axum::{
    body::Body,
    extract::{Multipart, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Json, Redirect, Response,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use ipp_printer::FamilySharedState;
use pdf_forms::{HouseholdFamilyProfile, PrintedFormJob};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
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

async fn index_handler(State(state): State<FamilySharedState>, headers: HeaderMap) -> Response {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    match host.as_str() {
        "home.aye" | "sanctuary.aye" | "aye" => {
            let st = state.library.status();
            Html(ui::render_welcome_html(
                &st,
                state.http_port,
                state.ipp_port,
                state.dns.port,
                &state.dns.lan_ipv4.to_string(),
            ))
            .into_response()
        }
        "music.aye" => Redirect::temporary("/?tab=phoenix").into_response(),
        "radar.aye" | "transparency.aye" | "alerts.aye" => {
            Redirect::temporary("/?tab=transparency").into_response()
        }
        "router.aye" | "gateway.aye" | "cache.aye" => {
            Redirect::temporary("/?tab=gateway").into_response()
        }
        "kitchen.aye" => Redirect::temporary("/?tab=kitchen").into_response(),
        "print.aye" => Redirect::temporary("/?tab=forms").into_response(),
        "ask.aye" => Redirect::temporary("/?tab=brain").into_response(),
        _ => Html(ui::render_family_hub_html(state.ipp_port)).into_response(),
    }
}

async fn welcome_handler(State(state): State<FamilySharedState>) -> Html<String> {
    let st = state.library.status();
    Html(ui::render_welcome_html(
        &st,
        state.http_port,
        state.ipp_port,
        state.dns.port,
        &state.dns.lan_ipv4.to_string(),
    ))
}

async fn captive_portal_handler(State(state): State<FamilySharedState>) -> Response {
    let port_suffix = if state.http_port == 80 {
        String::new()
    } else {
        format!(":{}", state.http_port)
    };
    Redirect::temporary(&format!("http://home.aye{port_suffix}/welcome")).into_response()
}

#[derive(Deserialize)]
struct CorpusQuery {
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn sanctuary_manifest_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let st = state.library.status();
    let docs = state.library.all_summaries();
    Json(serde_json::json!({
        "ok": true,
        "schema": "ayefire_sanctuary_manifest_v1",
        "compatible_with": ["8a.is", "8b.is", "tai.is", "ayefire"],
        "ethos": st.ethos,
        "total_docs": st.total_docs,
        "total_bytes": st.total_bytes,
        "merkle_root_blake3": st.merkle_root_blake3,
        "offline_only": st.offline_only,
        "no_tracking": st.no_tracking,
        "zero_float_enforced": st.zero_float_enforced,
        "dns": {
            "udp_port": state.dns.port,
            "lan_ipv4": state.dns.lan_ipv4.to_string(),
            "offline_portal": state.dns.offline_portal,
            "queries_answered": state.dns.queries_answered.load(std::sync::atomic::Ordering::Relaxed),
            "domains": ["home.aye", "library.aye", "learn.aye", "ask.aye", "music.aye", "kitchen.aye", "print.aye"]
        },
        "corpus_jsonl_url": "/api/sanctuary/corpus.jsonl",
        "documents": docs
    }))
}

async fn sanctuary_corpus_jsonl_handler(
    State(state): State<FamilySharedState>,
    Query(q): Query<CorpusQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    let body = state
        .library
        .export_corpus_jsonl(q.collection.as_deref(), limit);
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/x-ndjson; charset=utf-8".to_string(),
        )],
        body,
    )
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

#[derive(Deserialize)]
struct LibrarySearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn library_status_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "library": state.library.status()
    }))
}

async fn library_search_handler(
    State(state): State<FamilySharedState>,
    Query(params): Query<LibrarySearchQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(24).clamp(1, 60);
    let hits = state.library.search(
        &params.q,
        params.collection.as_deref(),
        params.topic.as_deref(),
        limit,
    );
    Json(serde_json::json!({
        "ok": true,
        "query": params.q,
        "count": hits.len(),
        "hits": hits
    }))
}

async fn library_doc_json_handler(
    State(state): State<FamilySharedState>,
    AxumPath(id): AxumPath<u32>,
) -> impl IntoResponse {
    match state.library.read_doc(id) {
        Some(full) => (StatusCode::OK, Json(serde_json::json!({ "ok": true, "document": full }))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "ok": false, "error": "manuscript not found" }))).into_response(),
    }
}

async fn library_reader_html_handler(
    State(state): State<FamilySharedState>,
    AxumPath(id): AxumPath<u32>,
) -> impl IntoResponse {
    match state.library.read_doc(id) {
        Some(full) => (StatusCode::OK, Html(sanctuary_library::render_reader_html(&full))).into_response(),
        None => (StatusCode::NOT_FOUND, Html("<h1>Manuscript not found</h1><p><a href=\"/\">Back to AyeFire Sanctuary</a></p>".to_string())).into_response(),
    }
}

async fn library_pdf_handler(
    State(state): State<FamilySharedState>,
    AxumPath(name): AxumPath<String>,
) -> impl IntoResponse {
    match state.library.read_allowlisted_pdf(&name) {
        Some((safe_name, bytes)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/pdf".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("inline; filename=\"{safe_name}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "PDF not in offline Sanctuary allowlist").into_response(),
    }
}

#[derive(Deserialize)]
struct LibraryAskPayload {
    question: String,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    topic: Option<String>,
}

async fn library_ask_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<LibraryAskPayload>,
) -> impl IntoResponse {
    let q = req.question.trim();
    if q.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": "question cannot be empty" })),
        );
    }

    let hits = state.library.search(
        q,
        req.collection.as_deref(),
        req.topic.as_deref(),
        4,
    );

    let mut context_block = String::new();
    for (idx, h) in hits.iter().enumerate() {
        context_block.push_str(&format!(
            "[{}] {} ({} · {})\nExcerpt: {}\n\n",
            idx + 1,
            h.doc.title,
            h.doc.collection,
            h.doc.rel_path,
            h.excerpt
        ));
    }

    let sys = "You are Aye, a calm, respectful offline tutor inside the AyeFire Sanctuary Internet (Education, Understanding, Love, Respect, Space). Ground your answer ONLY in the provided local library excerpts, cite sources as [1], [2], etc., keep observations distinct from speculation, and never invent references.";
    let prompt = if context_block.is_empty() {
        format!(
            "No exact local manuscript matched the query terms. Answer gently and honestly as a local offline tutor.\nQuestion: {q}"
        )
    } else {
        format!(
            "Local Sanctuary Library passages:\n{context_block}\nLearner question: {q}\nGive a clear, warm, accurate explanation citing [1]..[N] where relevant."
        )
    };

    match deepseek_pq::generate_local_reply(
        &state.llm_endpoint,
        &state.llm_model,
        sys,
        &prompt,
        280,
    )
    .await
    {
        Ok(reply) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "ai_available": true,
                "reply": reply,
                "citations": hits
            })),
        ),
        Err(err) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "ai_available": false,
                "ai_note": err,
                "reply": "Local DeepSeek p/q inference daemon is currently resting — showing the exact matching passages from your offline Sanctuary Library below so you can read the primary sources directly.",
                "citations": hits
            })),
        ),
    }
}

#[derive(Deserialize)]
struct PhoenixStreamQuery {
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    rate: Option<u32>,
    #[serde(default)]
    bits: Option<u16>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    cache: Option<u8>,
}

#[derive(Deserialize)]
struct CacheFetchQuery {
    url: String,
    #[serde(default)]
    refresh: Option<u8>,
}

#[derive(Deserialize)]
struct CacheWarmPayload {
    url: String,
}

async fn wpad_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let lan = state.dns.lan_ipv4;
    let proxy_port = state.edge_cache.proxy_port;
    let pac = format!(
        "function FindProxyForURL(url, host) {{\n  if (isPlainHostName(host) || dnsDomainIs(host, \".aye\") || dnsDomainIs(host, \".local\") || isInNet(host, \"172.16.47.0\", \"255.255.255.128\") || isInNet(host, \"127.0.0.0\", \"255.0.0.0\")) {{\n    return \"DIRECT\";\n  }}\n  if (url.substring(0, 5) == \"http:\") {{\n    return \"PROXY {lan}:{proxy_port}; DIRECT\";\n  }}\n  return \"DIRECT\";\n}}\n"
    );
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/x-ns-proxy-autoconfig; charset=utf-8".to_string(),
        )],
        pac,
    )
}

async fn phoenix_status_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let st = state.phoenix.status().await;
    Json(serde_json::json!({ "ok": true, "phoenix": st }))
}

async fn phoenix_tracks_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let tracks = state.phoenix.list_tracks().await;
    Json(serde_json::json!({
        "ok": true,
        "count": tracks.len(),
        "tracks": tracks
    }))
}

async fn phoenix_rescan_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let count = state.phoenix.rescan_tracks().await;
    Json(serde_json::json!({ "ok": true, "tracks_indexed": count }))
}

async fn serve_cached_wav_with_range(
    wav_path: PathBuf,
    req_headers: &HeaderMap,
    preset: &str,
    rate: u32,
) -> Response {
    let Ok(md) = tokio::fs::metadata(&wav_path).await else {
        return (StatusCode::NOT_FOUND, "cached audio missing").into_response();
    };
    let total_len = md.len();
    let range_hdr = req_headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());

    let (status, start, end) = match range_hdr
        .and_then(|h| phoenix_gateway::parse_http_byte_range(h, total_len))
    {
        Some((s, e)) => (StatusCode::PARTIAL_CONTENT, s, e),
        None => (StatusCode::OK, 0, total_len.saturating_sub(1)),
    };
    let send_len = end.saturating_sub(start).saturating_add(1);

    let Ok(mut file) = tokio::fs::File::open(&wav_path).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "failed opening cached wav").into_response();
    };
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return (StatusCode::RANGE_NOT_SATISFIABLE, "invalid byte range").into_response();
    }

    let stream = futures::stream::unfold((file, send_len), |(mut f, mut rem)| async move {
        if rem == 0 {
            return None;
        }
        let chunk_sz = (rem as usize).min(65_536);
        let mut buf = vec![0u8; chunk_sz];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                rem = rem.saturating_sub(n as u64);
                Some((Ok::<Vec<u8>, std::io::Error>(buf), (f, rem)))
            }
            Err(e) => Some((Err(e), (f, 0))),
        }
    });

    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, send_len.to_string())
        .header("X-AyeFire-Phoenix-Cache", "HIT")
        .header("X-AyeFire-Phoenix-Preset", preset)
        .header("X-AyeFire-Phoenix-Rate", rate.to_string());
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total_len}"),
        );
    }
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn phoenix_local_stream_handler(
    State(state): State<FamilySharedState>,
    AxumPath(id): AxumPath<String>,
    Query(q): Query<PhoenixStreamQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(track) = state.phoenix.find_track(&id).await else {
        return (StatusCode::NOT_FOUND, "track not found in local library").into_response();
    };
    let allow_cache = q.cache.unwrap_or(1) != 0;
    let opts = phoenix_gateway::StreamOptions::parse(
        q.preset.as_deref(),
        q.mode.as_deref(),
        q.rate,
        q.bits,
        allow_cache,
    );
    let fingerprint = format!("local:{}:{}", track.id, track.size_bytes);
    let cache_key = state.phoenix.cache_key(&fingerprint, opts);

    if opts.allow_cache {
        if let Some((wav_path, meta)) = state.phoenix.lookup_cached(&cache_key) {
            let sz = tokio::fs::metadata(&wav_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            state
                .phoenix
                .record_cache_hit(&track.title, opts, sz, meta.as_ref())
                .await;
            return serve_cached_wav_with_range(
                wav_path,
                &headers,
                opts.preset_label(),
                opts.sample_rate,
            )
            .await;
        }
    }

    let rx = match state.phoenix.spawn_live_stream(
        track.path.display().to_string(),
        track.title.clone(),
        opts,
        if opts.allow_cache {
            Some(cache_key)
        } else {
            None
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Phoenix live pipeline failed: {e}"),
            )
                .into_response();
        }
    };

    let stream = futures::stream::unfold(rx, |mut r| async move {
        r.recv()
            .await
            .map(|chunk| (Ok::<Vec<u8>, std::io::Error>(chunk), r))
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-AyeFire-Phoenix-Cache", "LIVE-DSP")
        .header("X-AyeFire-Phoenix-Preset", opts.preset_label())
        .header("X-AyeFire-Phoenix-Rate", opts.sample_rate.to_string())
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn phoenix_proxy_stream_handler(
    State(state): State<FamilySharedState>,
    Query(q): Query<PhoenixStreamQuery>,
    headers: HeaderMap,
) -> Response {
    let raw_url = q.url.unwrap_or_default();
    let url = match phoenix_gateway::validate_stream_url(&raw_url) {
        Ok(u) => u,
        Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
    };
    let allow_cache = q.cache.unwrap_or(1) != 0;
    let opts = phoenix_gateway::StreamOptions::parse(
        q.preset.as_deref(),
        q.mode.as_deref(),
        q.rate,
        q.bits,
        allow_cache,
    );
    let fingerprint = format!("url:{url}");
    let cache_key = state.phoenix.cache_key(&fingerprint, opts);

    if opts.allow_cache {
        if let Some((wav_path, meta)) = state.phoenix.lookup_cached(&cache_key) {
            let sz = tokio::fs::metadata(&wav_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            state
                .phoenix
                .record_cache_hit(&url, opts, sz, meta.as_ref())
                .await;
            return serve_cached_wav_with_range(
                wav_path,
                &headers,
                opts.preset_label(),
                opts.sample_rate,
            )
            .await;
        }
    }

    let rx = match state.phoenix.spawn_live_stream(
        url.clone(),
        url.clone(),
        opts,
        if opts.allow_cache {
            Some(cache_key)
        } else {
            None
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Phoenix live URL proxy failed: {e}"),
            )
                .into_response();
        }
    };

    let stream = futures::stream::unfold(rx, |mut r| async move {
        r.recv()
            .await
            .map(|chunk| (Ok::<Vec<u8>, std::io::Error>(chunk), r))
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-AyeFire-Phoenix-Cache", "LIVE-DSP")
        .header("X-AyeFire-Phoenix-Preset", opts.preset_label())
        .header("X-AyeFire-Phoenix-Rate", opts.sample_rate.to_string())
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[derive(Deserialize)]
struct InspectPolicyPayload {
    #[serde(default)]
    inspect_enabled: Option<bool>,
    #[serde(default)]
    auto_phoenix_audio: Option<bool>,
    #[serde(default)]
    privacy_bypass_enabled: Option<bool>,
    #[serde(default)]
    sinkhole_connect_enabled: Option<bool>,
    #[serde(default)]
    phoenix_preset: Option<String>,
    #[serde(default)]
    phoenix_rate: Option<u32>,
}

#[derive(Deserialize)]
struct RadarBlockPayload {
    host: String,
    #[serde(default = "default_true")]
    block: bool,
}

#[derive(Deserialize)]
struct RadarSilencePathPayload {
    host: String,
    path_prefix: String,
    #[serde(default = "default_true")]
    enable: bool,
}

#[derive(Deserialize)]
struct RadarAutoSilencePayload {
    enabled: bool,
}

#[derive(Deserialize)]
struct RadarExplainPayload {
    host: String,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct Id1PassportPatchPayload {
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    send_gpc_header: Option<bool>,
    #[serde(default)]
    send_id1p_header: Option<bool>,
}

async fn id1_passport_get_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let st = state.id1.status().await;
    Json(serde_json::json!({ "ok": true, "id1": st }))
}

async fn id1_passport_post_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<Id1PassportPatchPayload>,
) -> impl IntoResponse {
    let st = state
        .id1
        .update_config(
            req.handle,
            req.preset,
            req.send_gpc_header,
            req.send_id1p_header,
        )
        .await;
    Json(serde_json::json!({ "ok": true, "id1": st }))
}

async fn id1_attest_host_handler(
    State(state): State<FamilySharedState>,
    AxumPath(host): AxumPath<String>,
) -> impl IntoResponse {
    let norm = transparency_radar::normalize_host(&host);
    let st = state.radar.status().await;
    let site = st
        .sites
        .into_iter()
        .find(|s| s.host == norm)
        .unwrap_or(transparency_radar::SiteTransparencyView {
            host: norm.clone(),
            trust_of_20: 20,
            trust_pq: "1/1".to_string(),
            verdict: "UNOBSERVED".to_string(),
            total_requests: 0,
            post_beacons: 0,
            pixel_or_204_beacons: 0,
            burst_events_10s: 0,
            burst_rate_pq: "0 pkt/10s".to_string(),
            silenced_count: 0,
            third_party_hosts: Vec::new(),
            called_by_sites: Vec::new(),
            sample_beacon_paths: Vec::new(),
            known_tracker_note: transparency_radar::known_tracker_reason(&norm)
                .map(|s| s.to_string()),
            is_blocked: false,
            last_seen_unix: pdf_forms::now_unix_secs(),
        });
    let receipt = state.id1.attest_site(&site).await;
    Json(serde_json::json!({ "ok": true, "receipt": receipt }))
}

async fn id1_export_policy_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let policy = state.id1.export_portable_policy(&state.radar).await;
    Json(serde_json::json!({ "ok": true, "policy": policy }))
}

async fn id1_import_policy_handler(
    State(state): State<FamilySharedState>,
    Json(policy): Json<id1_transparency::Id1PortablePolicy>,
) -> impl IntoResponse {
    let (hosts, paths) = state
        .id1
        .import_portable_policy(&policy, &state.radar)
        .await;
    let st = state.id1.status().await;
    Json(serde_json::json!({
        "ok": true,
        "imported_blocked_hosts": hosts,
        "imported_silenced_paths": paths,
        "id1": st
    }))
}

async fn transparency_status_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let st = state.radar.status().await;
    let id1 = state.id1.status().await;
    Json(serde_json::json!({ "ok": true, "radar": st, "id1": id1 }))
}

async fn transparency_events_sse_handler(
    State(state): State<FamilySharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.radar.subscribe_alerts();
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(alert) => {
                    let payload =
                        serde_json::to_string(&alert).unwrap_or_else(|_| "{}".to_string());
                    let ev = Event::default().event("alert").data(payload);
                    return Some((Ok::<Event, Infallible>(ev), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn transparency_block_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<RadarBlockPayload>,
) -> impl IntoResponse {
    let ok = state.radar.block_host(&req.host, req.block).await;
    let st = state.radar.status().await;
    Json(serde_json::json!({ "ok": ok, "radar": st }))
}

async fn transparency_silence_path_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<RadarSilencePathPayload>,
) -> impl IntoResponse {
    let ok = state
        .radar
        .silence_path(&req.host, &req.path_prefix, req.enable)
        .await;
    let st = state.radar.status().await;
    Json(serde_json::json!({ "ok": ok, "radar": st }))
}

async fn transparency_auto_silence_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<RadarAutoSilencePayload>,
) -> impl IntoResponse {
    let enabled = state.radar.set_auto_silence(req.enabled).await;
    let st = state.radar.status().await;
    Json(serde_json::json!({ "ok": true, "auto_silence_beacons": enabled, "radar": st }))
}

async fn transparency_test_alert_handler(
    State(state): State<FamilySharedState>,
) -> impl IntoResponse {
    let alert = state.radar.emit_test_alert().await;
    Json(serde_json::json!({ "ok": true, "alert": alert }))
}

async fn transparency_explain_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<RadarExplainPayload>,
) -> impl IntoResponse {
    let (sanitized_summary, fallback) = state
        .radar
        .build_sanitized_explain_summary(&req.host)
        .await;
    let sys = "You are Aye, Hue's calm, honest local privacy advisor running on literal-loopback DeepSeek p/q inside AyeFire. Explain observed browser telemetry in 2-3 clear plain-English sentences. Distinguish normal site functionality from mouse-tracking, 1x1 pixels, or cross-site identity brokers without alarmism. Recommend whether to monitor, silence just the beacon path (keeps the site working!), or block the host.";
    let prompt = format!(
        "Sanitized local network observation (zero cookies, zero payloads):\n{sanitized_summary}\n\nExplain plainly what this host is doing and the smartest action for the user."
    );
    match deepseek_pq::generate_local_reply(
        &state.llm_endpoint,
        &state.llm_model,
        sys,
        &prompt,
        220,
    )
    .await
    {
        Ok(reply) => Json(serde_json::json!({
            "ok": true,
            "ai_available": true,
            "model": state.llm_model,
            "host": req.host,
            "explanation": reply,
            "evidence": sanitized_summary,
        })),
        Err(err) => Json(serde_json::json!({
            "ok": true,
            "ai_available": false,
            "ai_note": err,
            "host": req.host,
            "explanation": fallback,
            "evidence": sanitized_summary,
        })),
    }
}

async fn gateway_ca_cert_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/x-x509-ca-cert".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"ayefire-local-ca.crt\"".to_string(),
            ),
        ],
        state.inspect.ca_cert_pem.clone(),
    )
}

async fn gateway_inspect_get_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let st = state.inspect.status().await;
    Json(serde_json::json!({ "ok": true, "inspect": st }))
}

async fn gateway_inspect_post_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<InspectPolicyPayload>,
) -> impl IntoResponse {
    let _ = state
        .inspect
        .update_policy(
            req.inspect_enabled,
            req.auto_phoenix_audio,
            req.privacy_bypass_enabled,
            req.sinkhole_connect_enabled,
            req.phoenix_preset,
            req.phoenix_rate,
        )
        .await;
    let st = state.inspect.status().await;
    Json(serde_json::json!({ "ok": true, "inspect": st }))
}

async fn gateway_status_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let hw = gateway_status::inspect_gateway_hardware();
    let edge = state.edge_cache.status().await;
    let phx = state.phoenix.status().await;
    let insp = state.inspect.status().await;
    let dns_cache_entries = state.dns.cache_len().await;

    Json(serde_json::json!({
        "ok": true,
        "schema": "ayefire_gateway_v1",
        "hardware": hw,
        "inspect": insp,
        "dns": {
            "port": state.dns.port,
            "lan_ipv4": state.dns.lan_ipv4.to_string(),
            "mode": if state.dns.gateway_mode { "gateway+cache" } else { "airgap" },
            "sinkhole_enabled": state.dns.sinkhole_enabled,
            "upstream_primary": state.dns.upstream_primary.to_string(),
            "wan_dns_reachable": state.dns.wan_dns_reachable.load(Ordering::Relaxed),
            "queries_answered": state.dns.queries_answered.load(Ordering::Relaxed),
            "aye_local_hits": state.dns.aye_local_hits.load(Ordering::Relaxed),
            "cache_hits": state.dns.cache_hits.load(Ordering::Relaxed),
            "upstream_forwards": state.dns.upstream_forwards.load(Ordering::Relaxed),
            "sinkhole_blocked": state.dns.sinkhole_blocked.load(Ordering::Relaxed),
            "cache_entries": dns_cache_entries,
        },
        "edge_cache": edge,
        "phoenix_cache": {
            "entries": phx.cached_entries,
            "cached_mib": phx.cached_mib,
            "hits": phx.cache_hits,
            "misses": phx.cache_misses,
            "hit_ratio_pq": phx.hit_ratio_pq,
            "wan_bytes_saved": phx.wan_bytes_saved,
            "frames_restored": phx.frames_restored,
        }
    }))
}

async fn edge_cache_fetch_handler(
    State(state): State<FamilySharedState>,
    Query(q): Query<CacheFetchQuery>,
) -> Response {
    let force_refresh = q.refresh.unwrap_or(0) != 0;
    match state
        .edge_cache
        .fetch_or_serve(&q.url, true, force_refresh)
        .await
    {
        Ok(res) => Response::builder()
            .status(StatusCode::from_u16(res.status_code).unwrap_or(StatusCode::OK))
            .header(header::CONTENT_TYPE, res.content_type)
            .header("X-AyeFire-Cache", res.cache_status)
            .header("X-AyeFire-Blake3", res.content_blake3)
            .body(Body::from(res.body))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Err(e) => (StatusCode::BAD_GATEWAY, format!("Cache fetch error: {e}")).into_response(),
    }
}

async fn edge_cache_warm_handler(
    State(state): State<FamilySharedState>,
    Json(req): Json<CacheWarmPayload>,
) -> impl IntoResponse {
    match state.edge_cache.fetch_or_serve(&req.url, true, false).await {
        Ok(res) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "url": req.url,
                "cache_status": res.cache_status,
                "http_status": res.status_code,
                "content_type": res.content_type,
                "size_bytes": res.body.len(),
                "blake3_key": res.blake3_key,
                "content_blake3": res.content_blake3,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

async fn edge_cache_objects_handler(State(state): State<FamilySharedState>) -> impl IntoResponse {
    let objects = state.edge_cache.list_cached_objects(48).await;
    Json(serde_json::json!({
        "ok": true,
        "count": objects.len(),
        "objects": objects
    }))
}

async fn offline_sanctuary_headers(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' blob: data:; object-src 'self'; frame-ancestors 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    resp
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

    let library = Arc::new(sanctuary_library::SanctuaryLibrary::load_from_env());
    let lib_st = library.status();
    println!(
        "[SANCTUARY-LIBRARY] docs={} terms={} size={}MiB pdfs={} merkle={}.. (offline=true no_tracking=true)",
        lib_st.total_docs,
        lib_st.total_indexed_terms,
        lib_st.total_mib,
        lib_st.pdfs.len(),
        &lib_st.merkle_root_blake3[..lib_st.merkle_root_blake3.len().min(16)]
    );

    let pq_status = deepseek_pq::inspect_local_pqm_bundle(&bundle_dir, &llm_endpoint, &llm_model);
    println!(
        "[DEEPSEEK-PQM] bundle_present={} matrices={}/{} size={}MiB merkle={}..",
        pq_status.bundle_present,
        pq_status.ternary_matrices_found,
        pq_status.ternary_matrices_expected,
        pq_status.total_bundle_mib,
        &pq_status.merkle_root_blake3[..pq_status.merkle_root_blake3.len().min(16)]
    );

    let merkle_short = &lib_st.merkle_root_blake3[..lib_st.merkle_root_blake3.len().min(16)];
    let radar = transparency_radar::TransparencyRadar::load_from_env();
    let dns = sanctuary_dns::SanctuaryDnsConfig::from_env(
        lib_st.total_docs,
        merkle_short,
        radar.shared_blocked_hosts(),
    );
    let phoenix = phoenix_gateway::PhoenixGateway::load_from_env();
    let edge_cache = edge_cache::EdgeCache::load_from_env();
    let inspect = tls_inspect::TlsInspectState::load_or_create_from_env()?;
    let id1 = id1_transparency::Id1TransparencyBridge::load_from_env();

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
        library,
        dns: dns.clone(),
        phoenix: phoenix.clone(),
        edge_cache: edge_cache.clone(),
        inspect: inspect.clone(),
        radar: radar.clone(),
        id1: id1.clone(),
        http_port,
    };

    // Spawn pure-Rust Sovereign `.aye` + Caching Forwarding DNS responder
    let dns_cfg = dns.clone();
    tokio::spawn(async move {
        if let Err(e) = sanctuary_dns::spawn_dns_responder(dns_cfg).await {
            eprintln!("[SANCTUARY-DNS] responder exited: {e}");
        }
    });

    // Spawn LAN BLAKE3 HTTP Pull-Through Cache + Tier-1/Tier-3 HTTPS Inspect Proxy (:8792)
    let proxy_cache = edge_cache.clone();
    let proxy_inspect = inspect.clone();
    let proxy_phoenix = phoenix.clone();
    let proxy_radar = radar.clone();
    let proxy_id1 = id1.clone();
    tokio::spawn(async move {
        if let Err(e) = edge_cache::spawn_http_cache_proxy(
            proxy_cache,
            proxy_inspect,
            proxy_phoenix,
            proxy_radar,
            proxy_id1,
        )
        .await
        {
            eprintln!("[EDGE-PROXY] proxy exited: {e}");
        }
    });

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
        .route("/welcome", get(welcome_handler))
        .route("/library", get(index_handler))
        .route("/read/:id", get(library_reader_html_handler))
        // OS Captive-Portal Probes (warmly land offline Wi-Fi guests in the Sanctuary!)
        .route("/generate_204", get(captive_portal_handler))
        .route("/gen_204", get(captive_portal_handler))
        .route("/hotspot-detect.html", get(captive_portal_handler))
        .route("/connecttest.txt", get(captive_portal_handler))
        .route("/canonical.html", get(captive_portal_handler))
        .route("/check_network_status.txt", get(captive_portal_handler))
        .route("/ncsi.txt", get(captive_portal_handler))
        // Public Commons (8a.is / 8b.is / tai.is) Manifest & JSONL Corpus API
        .route("/api/sanctuary/manifest", get(sanctuary_manifest_handler))
        .route("/api/sanctuary/corpus.jsonl", get(sanctuary_corpus_jsonl_handler))
        // WPAD LAN Proxy Auto-Config + AyeFire Live Gateway + BLAKE3 Edge Cache
        .route("/wpad.dat", get(wpad_handler))
        .route("/api/gateway/status", get(gateway_status_handler))
        .route("/api/gateway/ca.crt", get(gateway_ca_cert_handler))
        .route(
            "/api/gateway/inspect",
            get(gateway_inspect_get_handler).post(gateway_inspect_post_handler),
        )
        // Live Transparency Radar, SSE Risk Notifications & Local DeepSeek p/q Risk Explainer
        .route("/api/transparency/status", get(transparency_status_handler))
        .route("/api/transparency/events", get(transparency_events_sse_handler))
        .route("/api/transparency/block", post(transparency_block_handler))
        .route(
            "/api/transparency/silence-path",
            post(transparency_silence_path_handler),
        )
        .route(
            "/api/transparency/auto-silence",
            post(transparency_auto_silence_handler),
        )
        .route(
            "/api/transparency/test-alert",
            post(transparency_test_alert_handler),
        )
        .route("/api/transparency/explain", post(transparency_explain_handler))
        // id1.is Sovereign Transparency Passport, 16-Byte ID1P & Portable Signed Policy Sync
        .route(
            "/api/id1/transparency/passport",
            get(id1_passport_get_handler).post(id1_passport_post_handler),
        )
        .route(
            "/api/id1/transparency/attest/:host",
            get(id1_attest_host_handler),
        )
        .route(
            "/api/id1/transparency/policy",
            get(id1_export_policy_handler),
        )
        .route(
            "/api/id1/transparency/import-policy",
            post(id1_import_policy_handler),
        )
        .route("/api/cache/fetch", get(edge_cache_fetch_handler))
        .route("/api/cache/warm", post(edge_cache_warm_handler))
        .route("/api/cache/objects", get(edge_cache_objects_handler))
        // On-the-Fly Zero-Float Phoenix Audio Gateway & BLAKE3 Restoration Cache
        .route("/api/phoenix/status", get(phoenix_status_handler))
        .route("/api/phoenix/tracks", get(phoenix_tracks_handler))
        .route("/api/phoenix/rescan", post(phoenix_rescan_handler))
        .route("/api/phoenix/local/:id", get(phoenix_local_stream_handler))
        .route("/api/phoenix/proxy", get(phoenix_proxy_stream_handler))
        .route("/api/family/status", get(family_status_handler))
        .route("/api/library/status", get(library_status_handler))
        .route("/api/library/search", get(library_search_handler))
        .route("/api/library/doc/:id", get(library_doc_json_handler))
        .route("/api/library/ask", post(library_ask_handler))
        .route("/api/library/pdf/:name", get(library_pdf_handler))
        .route("/api/kitchen/toggle-oven", post(toggle_oven_handler))
        .route("/api/kitchen/ask", post(ask_kitchen_handler))
        .route("/api/brain/ask", post(ask_brain_handler))
        .route("/api/forms/test-ipp-victor", post(test_ipp_victor_handler))
        .route("/api/forms/upload", post(upload_pdf_handler))
        .route("/api/forms/:id/update", post(update_form_handler))
        .route("/api/forms/:id/filled.pdf", get(download_filled_pdf_handler))
        .route("/api/forms/:id/draft.eml", get(download_draft_eml_handler))
        .route("/api/profile", post(save_profile_handler))
        .layer(middleware::from_fn(offline_sanctuary_headers))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], http_port));
    println!("[FAMILY-HUB] Serving AyeFire Family Care Hub on http://0.0.0.0:{http_port}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
