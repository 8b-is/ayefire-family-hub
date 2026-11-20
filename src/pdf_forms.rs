#![deny(clippy::float_arithmetic)]
//! Print-to-Aye PDF Form Extractor, Local Family Profile Matcher, `oxidize-pdf` Form Filler,
//! and Ready-to-Review `.eml` Email Draft Builder.
//!
//! Zero floating-point arithmetic in our logic — exact integer layout points converted via
//! `f64::from(...)` strictly at the `oxidize_pdf` rendering library call boundary.

use crate::deepseek_pq;
use oxidize_pdf::parser::{PdfDocument, PdfReader};
use oxidize_pdf::{Document, Font, Page};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_SPOOL_DIR: &str = "/home/hue/.local/share/ayefire/family-spool";
pub const DEFAULT_PROFILE_PATH: &str = "/home/hue/.config/ayefire/family-profile.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMemberRecord {
    pub name: String,
    pub role: String,
    pub dob: String,
    pub grade_or_notes: String,
    pub school_or_org: String,
    pub physician_name: String,
    pub physician_phone: String,
    pub insurance_provider: String,
    pub allergies: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HouseholdFamilyProfile {
    pub household_name: String,
    pub primary_guardian: String,
    pub primary_email: String,
    pub primary_phone: String,
    pub secondary_guardian: String,
    pub secondary_email: String,
    pub secondary_phone: String,
    pub home_address: String,
    pub emergency_contact_name: String,
    pub emergency_contact_phone: String,
    pub default_school_athletics_email: String,
    pub members: Vec<FamilyMemberRecord>,
}

impl Default for HouseholdFamilyProfile {
    fn default() -> Self {
        Self {
            household_name: "Hue & Alex Household (Local-Only Profile)".to_string(),
            primary_guardian: "Hue / Chris".to_string(),
            primary_email: "hue@8b.is".to_string(),
            primary_phone: "(Edit in Local Profile)".to_string(),
            secondary_guardian: "Alexandra".to_string(),
            secondary_email: "alex@8b.is".to_string(),
            secondary_phone: "(Edit in Local Profile)".to_string(),
            home_address: "(Edit local street address — stays on home disk only)".to_string(),
            emergency_contact_name: "Alexandra (Mom)".to_string(),
            emergency_contact_phone: "(Edit emergency phone)".to_string(),
            default_school_athletics_email: "athletics@school.example".to_string(),
            members: vec![
                FamilyMemberRecord {
                    name: "Victor".to_string(),
                    role: "Student Athlete".to_string(),
                    dob: "(Confirm DOB)".to_string(),
                    grade_or_notes: "Athletics & School Forms".to_string(),
                    school_or_org: "(Confirm School Name)".to_string(),
                    physician_name: "(Confirm Family Doctor)".to_string(),
                    physician_phone: "(Confirm Doctor Phone)".to_string(),
                    insurance_provider: "(Confirm Insurance Plan)".to_string(),
                    allergies: "None known (confirm before signing)".to_string(),
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormFieldDraft {
    pub key: String,
    pub label: String,
    pub value: String,
    pub needs_human_confirm: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintedFormJob {
    pub id: String,
    pub title: String,
    pub source_channel: String,
    pub received_unix: u64,
    pub original_pdf_path: String,
    pub filled_pdf_path: String,
    pub draft_eml_path: String,
    pub blake3_hex: String,
    pub page_count: u32,
    pub extracted_text_preview: String,
    pub matched_member: String,
    pub recipient_email: String,
    pub email_subject: String,
    pub fields: Vec<FormFieldDraft>,
    pub aye_summary: String,
    pub status: String,
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load_or_create_profile(path: &Path) -> HouseholdFamilyProfile {
    if let Ok(raw) = fs::read_to_string(path) {
        if let Ok(parsed) = serde_json::from_str::<HouseholdFamilyProfile>(&raw) {
            return parsed;
        }
    }
    let profile = HouseholdFamilyProfile::default();
    let _ = save_profile(path, &profile);
    profile
}

pub fn save_profile(path: &Path, profile: &HouseholdFamilyProfile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(profile)?;
    fs::write(&tmp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Extract text & page count using pure-Rust `oxidize-pdf`, falling back cleanly to `pdftotext`
/// if the PDF uses compressed streams that poppler decodes more cleanly.
pub fn extract_pdf_text(path: &Path) -> (u32, String) {
    let mut pages = 1u32;
    let mut text = String::new();

    if let Ok(reader) = PdfReader::open(path) {
        let doc = PdfDocument::new(reader);
        if let Ok(cnt) = doc.page_count() {
            pages = cnt.max(1);
        }
        if let Ok(extracted) = doc.extract_text() {
            for p in extracted {
                if !p.text.trim().is_empty() {
                    text.push_str(p.text.trim());
                    text.push('\n');
                }
            }
        }
    }

    if text.trim().len() < 24 {
        if let Ok(out) = Command::new("pdftotext").arg(path).arg("-").output() {
            if out.status.success() {
                let poppler_txt = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if poppler_txt.len() > text.trim().len() {
                    text = poppler_txt;
                }
            }
        }
    }

    (pages, text)
}

/// Pure-Rust generator for a realistic blank Victor Athletic Approval Form PDF so Hue can test
/// real end-to-end IPP printing (`Print-Job` over port `:6631`) in one click.
pub fn generate_sample_athletic_form_pdf(out_path: &Path) -> anyhow::Result<()> {
    let mut doc = Document::new();
    doc.set_title("Student Athletic Participation & Parental Approval Form");
    doc.set_author("School Athletics Department");

    let mut page = Page::letter();
    let lines: &[(i32, i32, bool, &str)] = &[
        (54, 730, true, "STUDENT ATHLETIC PARTICIPATION & PARENTAL APPROVAL FORM"),
        (54, 708, false, "Return completed & signed form to: athletics@school.example"),
        (54, 676, false, "Student Full Name: ____________________________________________"),
        (54, 650, false, "Date of Birth: ____________________   Grade: __________________"),
        (54, 624, false, "School Name: _________________________________________________"),
        (54, 598, false, "Sport / Activity: ____________________________________________"),
        (54, 566, true, "PARENT / GUARDIAN CONTACT & EMERGENCY AUTHORIZATION"),
        (54, 540, false, "Parent / Guardian Name: _______________________________________"),
        (54, 514, false, "Guardian Email: ______________________________________________"),
        (54, 488, false, "Guardian Phone: ______________________________________________"),
        (54, 462, false, "Home Address: ________________________________________________"),
        (54, 436, false, "Emergency Contact & Phone: ____________________________________"),
        (54, 404, true, "MEDICAL CLEARANCE & INSURANCE"),
        (54, 378, false, "Family Physician & Phone: _____________________________________"),
        (54, 352, false, "Health Insurance Provider: ____________________________________"),
        (54, 326, false, "Known Allergies / Medical Notes: ______________________________"),
        (54, 294, true, "CONSENT & SIGNATURE"),
        (54, 268, false, "[ ] I authorize emergency medical care if parent cannot be reached."),
        (54, 242, false, "Parent / Guardian Signature: __________________________________"),
        (54, 216, false, "Date Signed: ________________________"),
    ];

    for &(x, y, bold, text) in lines {
        let font = if bold { Font::HelveticaBold } else { Font::Helvetica };
        let size = if y == 730 { 14 } else if bold { 11 } else { 10 };
        page.text()
            .set_font(font, f64::from(size))
            .at(f64::from(x), f64::from(y))
            .write(text)?;
    }

    doc.add_page(page);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    doc.save(out_path)?;
    Ok(())
}

/// Generate a crisp filled summary/overlay PDF using pure-Rust `oxidize-pdf` (zero floats in logic).
pub fn render_filled_form_pdf(job: &PrintedFormJob, out_path: &Path) -> anyhow::Result<()> {
    let mut doc = Document::new();
    doc.set_title(&format!("{} — AyeFire Filled Draft", job.title));
    doc.set_author("AyeFire Sovereign Family Assistant (Local-Only)");

    let mut page = Page::letter();
    let mut y: i32 = 742;

    page.text()
        .set_font(Font::HelveticaBold, f64::from(14))
        .at(f64::from(48), f64::from(y))
        .write(&sanitize_ascii(&format!("COMPLETED FORM: {}", job.title)))?;
    y -= 20;

    page.text()
        .set_font(Font::Helvetica, f64::from(9))
        .at(f64::from(48), f64::from(y))
        .write(&sanitize_ascii(&format!(
            "Prepared locally by AyeFire (Zero Cloud) | Job {} | Source BLAKE3 {}..",
            job.id,
            &job.blake3_hex[..job.blake3_hex.len().min(16)]
        )))?;
    y -= 26;

    for f in &job.fields {
        if y < 96 {
            break;
        }
        let flag = if f.needs_human_confirm { " [CONFIRM]" } else { "" };
        let line = sanitize_ascii(&format!("{}{}: {}", f.label, flag, f.value));
        page.text()
            .set_font(Font::HelveticaBold, f64::from(10))
            .at(f64::from(48), f64::from(y))
            .write(&truncate_chars(&line, 94))?;
        y -= 20;
    }

    y -= 10;
    page.text()
        .set_font(Font::HelveticaBold, f64::from(10))
        .at(f64::from(48), f64::from(y.max(72)))
        .write("Parent / Guardian Signature (Human Review Required): ___________________________")?;
    y -= 20;
    page.text()
        .set_font(Font::Helvetica, f64::from(9))
        .at(f64::from(48), f64::from(y.max(52)))
        .write("Note: AyeFire never signs legal/medical approvals autonomously. Review all fields before sending.")?;

    doc.add_page(page);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    doc.save(out_path)?;
    Ok(())
}

/// Build an RFC 5322 `.eml` email draft with the filled PDF attached as standard base64 so
/// Hue or Alex can double-click / open in any mail client, review, and press Send.
pub fn write_email_draft_eml(
    job: &PrintedFormJob,
    profile: &HouseholdFamilyProfile,
    filled_pdf_path: &Path,
    eml_path: &Path,
) -> anyhow::Result<()> {
    let pdf_bytes = fs::read(filled_pdf_path)?;
    let b64 = base64_encode_mime(&pdf_bytes);
    let boundary = format!("----AyeFireFormDraft_{}", job.id.replace('-', ""));
    let filename = format!("{}-filled.pdf", slugify(&job.title));

    let mut body_lines = String::new();
    body_lines.push_str("Hello,\r\n\r\n");
    body_lines.push_str(&format!(
        "Attached is the completed {} for {}.\r\n\r\n",
        job.title, job.matched_member
    ));
    body_lines.push_str("Summary of filled fields:\r\n");
    for f in &job.fields {
        body_lines.push_str(&format!("  - {}: {}\r\n", f.label, f.value));
    }
    body_lines.push_str(&format!(
        "\r\nWarm regards,\r\n{} & {}\r\n{}\r\n",
        profile.primary_guardian, profile.secondary_guardian, profile.primary_email
    ));

    let eml = format!(
        "From: {} <{}>\r\n\
         To: {}\r\n\
         Subject: {}\r\n\
         MIME-Version: 1.0\r\n\
         X-Unsent: 1\r\n\
         Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: text/plain; charset=\"utf-8\"\r\n\
         Content-Transfer-Encoding: 7bit\r\n\
         \r\n\
         {body_lines}\r\n\
         --{boundary}\r\n\
         Content-Type: application/pdf; name=\"{filename}\"\r\n\
         Content-Disposition: attachment; filename=\"{filename}\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {b64}\r\n\
         --{boundary}--\r\n",
        sanitize_ascii(&profile.primary_guardian),
        sanitize_ascii(&profile.primary_email),
        sanitize_ascii(&job.recipient_email),
        sanitize_ascii(&job.email_subject),
    );

    fs::write(eml_path, eml)?;
    Ok(())
}

/// Fast synchronous phase (< 100ms): extract PDF text, match local household profile,
/// generate filled PDF via `oxidize-pdf`, and write the `.eml` email draft immediately so
/// IPP printers (iPhone AirPrint / Mac / CUPS) receive instant `successful-ok`.
pub fn process_incoming_pdf_fast(
    pdf_path: &Path,
    raw_title: &str,
    source_channel: &str,
    spool_dir: &Path,
    profile: &HouseholdFamilyProfile,
) -> anyhow::Result<(PrintedFormJob, String)> {
    let bytes = fs::read(pdf_path)?;
    let blake3_hex = blake3::hash(&bytes).to_hex().to_string();
    let (page_count, extracted) = extract_pdf_text(pdf_path);
    let lower = extracted.to_ascii_lowercase();

    let member = profile
        .members
        .iter()
        .find(|m| lower.contains(&m.name.to_ascii_lowercase()))
        .or_else(|| profile.members.first())
        .cloned()
        .unwrap_or_else(|| HouseholdFamilyProfile::default().members.remove(0));

    let title = if !raw_title.trim().is_empty() && raw_title != "Untitled" {
        raw_title.trim().to_string()
    } else if lower.contains("athletic") {
        format!("{}'s Athletic Approval Form", member.name)
    } else if lower.contains("permission") {
        format!("{}'s Permission Form", member.name)
    } else {
        "Printed Family Form".to_string()
    };

    let recipient_email = extract_first_email(&extracted)
        .unwrap_or_else(|| profile.default_school_athletics_email.clone());

    let fields = build_field_drafts(&lower, &member, profile);

    let needs_count = fields.iter().filter(|f| f.needs_human_confirm).count();
    let ready_count = fields.len().saturating_sub(needs_count);

    let default_summary = format!(
        "Got {} for {} ({} of {} fields pre-filled from your local family profile, {} waiting for your quick review & signature before emailing {}).",
        title, member.name, ready_count, fields.len(), needs_count, recipient_email
    );

    let id = format!(
        "form-{}-{}",
        now_unix_secs(),
        &blake3_hex[..blake3_hex.len().min(6)]
    );
    let filled_pdf_path = spool_dir.join(format!("{id}-filled.pdf"));
    let draft_eml_path = spool_dir.join(format!("{id}-draft.eml"));

    let job = PrintedFormJob {
        id: id.clone(),
        title: title.clone(),
        source_channel: source_channel.to_string(),
        received_unix: now_unix_secs(),
        original_pdf_path: pdf_path.display().to_string(),
        filled_pdf_path: filled_pdf_path.display().to_string(),
        draft_eml_path: draft_eml_path.display().to_string(),
        blake3_hex,
        page_count,
        extracted_text_preview: truncate_chars(extracted.trim(), 900),
        matched_member: member.name.clone(),
        recipient_email: recipient_email.clone(),
        email_subject: format!("Completed: {} ({})", title, member.name),
        fields,
        aye_summary: default_summary,
        status: "READY_FOR_REVIEW".to_string(),
    };

    render_filled_form_pdf(&job, &filled_pdf_path)?;
    write_email_draft_eml(&job, profile, &filled_pdf_path, &draft_eml_path)?;

    Ok((job, extracted))
}

/// Optional async enrichment via local DeepSeek `p/q` (loopback-only).
pub async fn summarize_form_with_deepseek(
    job: &PrintedFormJob,
    extracted_text: &str,
    llm_endpoint: &str,
    llm_model: &str,
) -> Option<String> {
    let needs_count = job.fields.iter().filter(|f| f.needs_human_confirm).count();
    let ready_count = job.fields.len().saturating_sub(needs_count);
    let sys_prompt = "You are Aye, a calm, practical sovereign home assistant running locally on Hue & Alex's AyeFire. Never invent private facts; summarize clearly in 2 short sentences.";
    let user_prompt = format!(
        "A form titled '{}' ({} page(s)) was just printed to Aye for {}. {} of {} fields matched the local household profile and {} need quick human confirmation before signing/emailing to {}. Extracted excerpt:\n{}\nGive Hue or Alex a warm, 2-sentence heads-up listing what is ready and what needs a quick check.",
        job.title,
        job.page_count,
        job.matched_member,
        ready_count,
        job.fields.len(),
        needs_count,
        job.recipient_email,
        truncate_chars(extracted_text, 700)
    );
    match deepseek_pq::generate_local_reply(llm_endpoint, llm_model, sys_prompt, &user_prompt, 120)
        .await
    {
        Ok(reply) if !reply.trim().is_empty() => Some(reply),
        _ => None,
    }
}

/// Combined helper for interactive browser uploads.
pub async fn process_incoming_pdf(
    pdf_path: &Path,
    raw_title: &str,
    source_channel: &str,
    spool_dir: &Path,
    profile: &HouseholdFamilyProfile,
    llm_endpoint: &str,
    llm_model: &str,
) -> anyhow::Result<PrintedFormJob> {
    let (mut job, extracted) =
        process_incoming_pdf_fast(pdf_path, raw_title, source_channel, spool_dir, profile)?;
    if let Some(summary) =
        summarize_form_with_deepseek(&job, &extracted, llm_endpoint, llm_model).await
    {
        job.aye_summary = summary;
    }
    Ok(job)
}

pub fn regenerate_job_artifacts(
    job: &mut PrintedFormJob,
    updates: &BTreeMap<String, String>,
    new_recipient: Option<&str>,
    profile: &HouseholdFamilyProfile,
) -> anyhow::Result<()> {
    for f in &mut job.fields {
        if let Some(v) = updates.get(&f.key) {
            f.value = v.trim().to_string();
            f.needs_human_confirm = f.value.is_empty() || f.value.contains("(Confirm") || f.value.contains("(Edit");
            f.source = "human_confirmed".to_string();
        }
    }
    if let Some(rcpt) = new_recipient {
        let r = rcpt.trim();
        if !r.is_empty() && r.contains('@') {
            job.recipient_email = r.to_string();
        }
    }
    let remaining = job.fields.iter().filter(|f| f.needs_human_confirm).count();
    job.status = if remaining == 0 {
        "CONFIRMED_READY_TO_SEND".to_string()
    } else {
        "UPDATED_NEEDS_REVIEW".to_string()
    };

    render_filled_form_pdf(job, Path::new(&job.filled_pdf_path))?;
    write_email_draft_eml(
        job,
        profile,
        Path::new(&job.filled_pdf_path),
        Path::new(&job.draft_eml_path),
    )?;
    Ok(())
}

fn build_field_drafts(
    lower_text: &str,
    member: &FamilyMemberRecord,
    profile: &HouseholdFamilyProfile,
) -> Vec<FormFieldDraft> {
    let mut fields = Vec::new();
    let add = |list: &mut Vec<FormFieldDraft>, key: &str, label: &str, val: &str, src: &str| {
        let needs = val.trim().is_empty() || val.contains("(Confirm") || val.contains("(Edit");
        list.push(FormFieldDraft {
            key: key.to_string(),
            label: label.to_string(),
            value: val.to_string(),
            needs_human_confirm: needs,
            source: src.to_string(),
        });
    };

    add(&mut fields, "student_name", "Student / Family Member Name", &member.name, "local_profile");
    if lower_text.contains("birth") || lower_text.contains("dob") || lower_text.is_empty() {
        add(&mut fields, "dob", "Date of Birth", &member.dob, "local_profile");
    }
    if lower_text.contains("school") || lower_text.contains("athletic") || lower_text.is_empty() {
        add(&mut fields, "school", "School / Organization", &member.school_or_org, "local_profile");
    }
    if lower_text.contains("sport") || lower_text.contains("activity") || lower_text.contains("athletic") {
        add(&mut fields, "sport", "Sport / Activity", "(Confirm Sport / Season)", "form_prompt");
    }
    add(&mut fields, "guardian_name", "Parent / Guardian", &format!("{} & {}", profile.primary_guardian, profile.secondary_guardian), "local_profile");
    add(&mut fields, "guardian_email", "Guardian Email", &profile.primary_email, "local_profile");
    add(&mut fields, "guardian_phone", "Guardian Phone", &profile.primary_phone, "local_profile");
    add(&mut fields, "home_address", "Home Address", &profile.home_address, "local_profile");
    add(
        &mut fields,
        "emergency_contact",
        "Emergency Contact",
        &format!("{} — {}", profile.emergency_contact_name, profile.emergency_contact_phone),
        "local_profile",
    );
    if lower_text.contains("physician") || lower_text.contains("doctor") || lower_text.contains("medical") || lower_text.contains("athletic") {
        add(
            &mut fields,
            "physician",
            "Family Physician",
            &format!("{} ({})", member.physician_name, member.physician_phone),
            "local_profile",
        );
        add(&mut fields, "insurance", "Health Insurance", &member.insurance_provider, "local_profile");
        add(&mut fields, "allergies", "Allergies / Medical Notes", &member.allergies, "local_profile");
    }
    add(&mut fields, "date_signed", "Approval Date", &current_utc_date_string(), "system_clock");
    fields
}

fn current_utc_date_string() -> String {
    // Pure integer Howard Hinnant civil-from-days conversion (zero floats!)
    let secs = now_unix_secs() as i64;
    let z = secs / 86_400 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", year, m, d)
}

fn extract_first_email(text: &str) -> Option<String> {
    for tok in text.split_whitespace() {
        let clean = tok.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '<' | '>' | '.' | '"' | '\''));
        if let Some((local, dom)) = clean.split_once('@') {
            if !local.is_empty() && dom.contains('.') && clean.len() <= 96 {
                return Some(clean.to_ascii_lowercase());
            }
        }
    }
    None
}

fn sanitize_ascii(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { ' ' })
        .collect()
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "form".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn base64_encode_mime(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() * 4) / 3 + (data.len() / 57) * 2 + 16);
    let mut col = 0usize;
    let mut i = 0usize;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        let c0 = TABLE[(b0 >> 2) as usize] as char;
        let c1 = TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char;
        let c2 = if i + 1 < data.len() {
            TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        };
        let c3 = if i + 2 < data.len() {
            TABLE[(b2 & 0x3F) as usize] as char
        } else {
            '='
        };

        for ch in [c0, c1, c2, c3] {
            out.push(ch);
            col += 1;
            if col == 76 {
                out.push_str("\r\n");
                col = 0;
            }
        }
        i += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_athletic_form_roundtrips_through_oxidize_pdf() {
        let tmp_dir = std::env::temp_dir().join("ayefire-pdf-test");
        let _ = fs::create_dir_all(&tmp_dir);
        let sample = tmp_dir.join("victor_athletic.pdf");
        generate_sample_athletic_form_pdf(&sample).expect("generate sample");
        let (pages, text) = extract_pdf_text(&sample);
        assert_eq!(pages, 1);
        assert!(text.contains("ATHLETIC") || text.contains("athletics@school.example"));
        let _ = fs::remove_file(&sample);
    }
}
