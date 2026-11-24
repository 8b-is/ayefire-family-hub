#![deny(clippy::float_arithmetic)]
//! AyeFire Sanctuary Offline Library — Education, Understanding, Love, Respect (`{<3,<3,<3}+1`).
//!
//! 100% local, zero-cloud, zero-tracking, zero-float full-text knowledge engine indexing:
//!   1. Sovereign Library (`sovereign-library`: books, Ultralove Editions, ONTOLOGY.md, Six Pillars)
//!   2. Curated Downloads Papers (`execution_is_not_validation.tex`, extracted PDFs, whitepapers)
//!   3. V3.1 Single-Source-of-Truth `standardgalactic` Canonical LaTeX Research (`2,488` papers)
//!
//! C64 memory discipline: full manuscript text is read on demand by exact `[byte_start..byte_end]`
//! offset from local volumes; RAM holds compact metadata + exact-integer inverted term index.

use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const DEFAULT_SOVEREIGN_LIBRARY_DIR: &str = "/home/hue/source/8b-is/sovereign-library";
pub const DEFAULT_CANONICAL_LATEX_DIR: &str =
    "/home/hue/.local/share/gdrive-digests/standardgalactic/canonical-latex";
pub const DEFAULT_DOWNLOADS_PAPERS_DIR: &str =
    "/home/hue/.local/share/gdrive-digests/sanctuary/downloads-papers";
pub const DEFAULT_DOWNLOADS_PDF_DIR: &str = "/home/hue/Downloads";

/// Strict allowlist of offline PDFs in `~/Downloads` served directly by `/api/library/pdf/:name`.
pub const ALLOWLISTED_PDFS: &[(&str, &str)] = &[
    (
        "evidentiary_authority.pdf",
        "Evidentiary Authority and the Acceleration of AI",
    ),
    (
        "execution_is_not_validation.pdf",
        "Execution Is Not Validation (Flyxion)",
    ),
    (
        "ecce_flyxion.pdf",
        "Ecce Flyxion — Philosophical Autobiography in Formal Specification",
    ),
    (
        "Evolving Dark Energy from the QCD Vacuum Technical Assessment.pdf",
        "Evolving Dark Energy from the QCD Vacuum: Technical Assessment",
    ),
    (
        "ayeos-uefi-bootloader-spec.pdf",
        "AyeOS Sovereign UEFI Bootloader Specification",
    ),
    (
        "Wave-Based_Cognitive_Encryption.pdf",
        "Wave-Based Cognitive Encryption (Visual Slide Deck)",
    ),
    (
        "AyeFire_Zero-Copy_Routing_Pipeline.pdf",
        "AyeFire Zero-Copy Routing Pipeline (Visual Slide Deck)",
    ),
];

const MAX_READ_BYTES: u64 = 524_288; // 512 KiB safe cap for single browser render
const MAX_BODY_TF_CAP: u16 = 24;

#[derive(Debug, Clone, Serialize)]
pub struct SanctuaryDocSummary {
    pub id: u32,
    pub collection: String,
    pub topic: String,
    pub repo: String,
    pub rel_path: String,
    pub title: String,
    pub byte_len: u64,
    pub snippet: String,
    pub blake3_short: String,
    pub pdf_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SanctuarySearchHit {
    pub doc: SanctuaryDocSummary,
    pub score: u64,
    pub matched_terms: usize,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SanctuaryDocFull {
    pub doc: SanctuaryDocSummary,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SanctuaryTopicStat {
    pub slug: String,
    pub label: String,
    pub description: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SanctuaryPdfItem {
    pub filename: String,
    pub title: String,
    pub byte_len: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SanctuaryStatus {
    pub schema: String,
    pub ethos: String,
    pub total_docs: usize,
    pub total_indexed_terms: usize,
    pub total_bytes: u64,
    pub total_mib: u64,
    pub collections: BTreeMap<String, usize>,
    pub topics: Vec<SanctuaryTopicStat>,
    pub pdfs: Vec<SanctuaryPdfItem>,
    pub merkle_root_blake3: String,
    pub offline_only: bool,
    pub no_tracking: bool,
    pub zero_float_enforced: bool,
}

#[derive(Debug, Clone)]
struct StoredDoc {
    summary: SanctuaryDocSummary,
    source_file_idx: usize,
    byte_start: u64,
    byte_end: u64,
}

#[derive(Debug, Clone, Copy)]
struct Posting {
    doc_id: u32,
    title_hits: u8,
    repo_hits: u8,
    body_tf: u16,
}

pub struct SanctuaryLibrary {
    source_files: Vec<PathBuf>,
    docs: Vec<StoredDoc>,
    inverted: HashMap<String, Vec<Posting>>,
    pdf_dir: PathBuf,
    pdfs: Vec<SanctuaryPdfItem>,
    collections: BTreeMap<String, usize>,
    topic_counts: BTreeMap<String, usize>,
    total_bytes: u64,
    merkle_root_blake3: String,
}

struct NewDoc<'a> {
    collection: &'a str,
    repo: &'a str,
    rel_path: &'a str,
    title: &'a str,
    source_file_idx: usize,
    byte_start: u64,
    byte_end: u64,
    text_for_index: &'a str,
    pdf_name: Option<String>,
}

impl SanctuaryLibrary {
    pub fn load_from_env() -> Self {
        let sov_dir = std::env::var("AYEFIRE_SOVEREIGN_LIBRARY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_SOVEREIGN_LIBRARY_DIR));
        let canon_dir = std::env::var("AYEFIRE_CANONICAL_LATEX_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CANONICAL_LATEX_DIR));
        let dl_papers_dir = std::env::var("AYEFIRE_DOWNLOADS_PAPERS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_DOWNLOADS_PAPERS_DIR));
        let pdf_dir = std::env::var("AYEFIRE_DOWNLOADS_PDF_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_DOWNLOADS_PDF_DIR));

        Self::load_from_dirs(&sov_dir, &dl_papers_dir, &canon_dir, &pdf_dir)
    }

    pub fn load_from_dirs(
        sovereign_dir: &Path,
        downloads_papers_dir: &Path,
        canonical_latex_dir: &Path,
        pdf_dir: &Path,
    ) -> Self {
        let mut lib = Self {
            source_files: Vec::new(),
            docs: Vec::new(),
            inverted: HashMap::new(),
            pdf_dir: pdf_dir.to_path_buf(),
            pdfs: Vec::new(),
            collections: BTreeMap::new(),
            topic_counts: BTreeMap::new(),
            total_bytes: 0,
            merkle_root_blake3: String::new(),
        };

        // 1. Sovereign Library (Education, Understanding, Love, Respect, Six Pillars)
        lib.ingest_sovereign_library(sovereign_dir);

        // 2. Curated ~/Downloads Papers (LaTeX + extracted PDFs + cryptography/Marine whitepapers)
        lib.ingest_downloads_papers(downloads_papers_dir);

        // 3. Curated V3.1 Canonical standardgalactic LaTeX Volumes
        lib.ingest_canonical_volumes(canonical_latex_dir);

        // 4. Allowlisted Offline PDFs (direct browser PDF viewing, zero path traversal)
        for (fname, title) in ALLOWLISTED_PDFS {
            let p = pdf_dir.join(fname);
            if let Ok(md) = fs::metadata(&p) {
                if md.is_file() {
                    lib.pdfs.push(SanctuaryPdfItem {
                        filename: (*fname).to_string(),
                        title: (*title).to_string(),
                        byte_len: md.len(),
                        url: format!("/api/library/pdf/{}", url_encode_segment(fname)),
                    });
                }
            }
        }

        // Compute deterministic BLAKE3 Merkle digest over all indexed manuscripts
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"AYEFIRE_SANCTUARY_LIBRARY_V1\n");
        for d in &lib.docs {
            hasher.update(d.summary.collection.as_bytes());
            hasher.update(b"|");
            hasher.update(d.summary.rel_path.as_bytes());
            hasher.update(b"|");
            hasher.update(&d.summary.byte_len.to_le_bytes());
            hasher.update(b"|");
            hasher.update(d.summary.blake3_short.as_bytes());
            hasher.update(b"\n");
        }
        lib.merkle_root_blake3 = hasher.finalize().to_hex().to_string();
        lib
    }

    pub fn status(&self) -> SanctuaryStatus {
        let topic_defs = [
            (
                "pillars",
                "Love, Respect & The Six Pillars",
                "Sovereign Library, Ultralove Editions, Ontology, Ethics, Admissibility & Repair",
            ),
            (
                "physics",
                "Physics, Cosmology & RSVP",
                "Relativistic Scalar-Vector-Plenum, Cosmology, Dark Energy, Thermodynamics & Skeptical Audits",
            ),
            (
                "systems",
                "Systems, Rust, Verification & AyeOS",
                "Exact Integer Inference, Spherepop OS, Cryptography, AyeFire & Formal Specs",
            ),
            (
                "mind",
                "Cognition, Memory & Sovereign AI",
                "Active Geodesic Inference, MEM|8, MEMNET, Alignment, CLIO & Interpretability",
            ),
            (
                "craft",
                "Math, Language, Society & Craft",
                "Calculus, Algebra, Standard Galactic Alphabet, Rhetoric, History & Coordination",
            ),
        ];

        let topics = topic_defs
            .iter()
            .map(|(slug, label, desc)| SanctuaryTopicStat {
                slug: (*slug).to_string(),
                label: (*label).to_string(),
                description: (*desc).to_string(),
                count: *self.topic_counts.get(*slug).unwrap_or(&0),
            })
            .collect();

        SanctuaryStatus {
            schema: "ayefire_sanctuary_library_v1".to_string(),
            ethos: "Education · Understanding · Love · Respect · Space ({<3,<3,<3}+1)".to_string(),
            total_docs: self.docs.len(),
            total_indexed_terms: self.inverted.len(),
            total_bytes: self.total_bytes,
            total_mib: self.total_bytes / (1024 * 1024),
            collections: self.collections.clone(),
            topics,
            pdfs: self.pdfs.clone(),
            merkle_root_blake3: self.merkle_root_blake3.clone(),
            offline_only: true,
            no_tracking: true,
            zero_float_enforced: true,
        }
    }

    pub fn search(
        &self,
        query: &str,
        collection_filter: Option<&str>,
        topic_filter: Option<&str>,
        limit: usize,
    ) -> Vec<SanctuarySearchHit> {
        let capped_limit = limit.clamp(1, 60);
        let q_terms = tokenize_query(query);

        // Empty query -> return curated featured shelf filtered by collection/topic
        if q_terms.is_empty() {
            return self
                .docs
                .iter()
                .filter(|d| matches_filters(d, collection_filter, topic_filter))
                .take(capped_limit)
                .map(|d| SanctuarySearchHit {
                    doc: d.summary.clone(),
                    score: 100,
                    matched_terms: 0,
                    excerpt: d.summary.snippet.clone(),
                })
                .collect();
        }

        let total_docs = (self.docs.len() as u64).max(1);
        // doc_id -> (exact_integer_score, matched_term_count)
        let mut scores: HashMap<u32, (u64, usize)> = HashMap::new();

        for term in &q_terms {
            if let Some(postings) = self.inverted.get(term) {
                let df = postings.len() as u64;
                // Exact integer IDF scaled by 100 (zero floats!)
                let idf_q100 = 100u64.saturating_add((total_docs.saturating_mul(100)) / (df + 1));
                for p in postings {
                    if let Some(d) = self.docs.get(p.doc_id as usize) {
                        if !matches_filters(d, collection_filter, topic_filter) {
                            continue;
                        }
                        let tf_weight = u64::from(p.title_hits)
                            .saturating_mul(14)
                            .saturating_add(u64::from(p.repo_hits).saturating_mul(7))
                            .saturating_add(u64::from(p.body_tf).saturating_mul(2));
                        let add_score = tf_weight.saturating_mul(idf_q100);
                        let entry = scores.entry(p.doc_id).or_insert((0, 0));
                        entry.0 = entry.0.saturating_add(add_score);
                        entry.1 = entry.1.saturating_add(1);
                    }
                }
            }
        }

        // Bonus for matching multiple distinct query terms (AND-like precision boost)
        let mut ranked: Vec<(u32, u64, usize)> = scores
            .into_iter()
            .map(|(doc_id, (base, matched))| {
                let multi_bonus = (matched as u64).saturating_mul(matched as u64).saturating_mul(250);
                (doc_id, base.saturating_add(multi_bonus), matched)
            })
            .collect();

        ranked.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.0.cmp(&b.0))
        });

        ranked
            .into_iter()
            .take(capped_limit)
            .filter_map(|(doc_id, score, matched)| {
                let d = self.docs.get(doc_id as usize)?;
                let excerpt = self
                    .read_doc_excerpt(d, &q_terms, 320)
                    .unwrap_or_else(|| d.summary.snippet.clone());
                Some(SanctuarySearchHit {
                    doc: d.summary.clone(),
                    score,
                    matched_terms: matched,
                    excerpt,
                })
            })
            .collect()
    }

    pub fn read_doc(&self, id: u32) -> Option<SanctuaryDocFull> {
        let d = self.docs.get(id as usize)?;
        let path = self.source_files.get(d.source_file_idx)?;
        let total = d.byte_end.saturating_sub(d.byte_start);
        let to_read = total.min(MAX_READ_BYTES);
        let truncated = total > MAX_READ_BYTES;

        let mut file = File::open(path).ok()?;
        file.seek(SeekFrom::Start(d.byte_start)).ok()?;
        let mut buf = Vec::with_capacity(to_read as usize);
        file.take(to_read).read_to_end(&mut buf).ok()?;
        let content = String::from_utf8_lossy(&buf).into_owned();

        Some(SanctuaryDocFull {
            doc: d.summary.clone(),
            content,
            truncated,
        })
    }

    pub fn all_summaries(&self) -> Vec<SanctuaryDocSummary> {
        self.docs.iter().map(|d| d.summary.clone()).collect()
    }

    pub fn export_corpus_jsonl(&self, collection_filter: Option<&str>, limit: usize) -> String {
        let capped = limit.clamp(1, 5000);
        let mut out = String::new();
        let mut count = 0usize;
        for d in &self.docs {
            if !matches_filters(d, collection_filter, None) {
                continue;
            }
            let Some(full) = self.read_doc(d.summary.id) else {
                continue;
            };
            let rec = serde_json::json!({
                "id": full.doc.id,
                "collection": full.doc.collection,
                "topic": full.doc.topic,
                "repo": full.doc.repo,
                "rel_path": full.doc.rel_path,
                "title": full.doc.title,
                "byte_len": full.doc.byte_len,
                "blake3": full.doc.blake3_short,
                "truncated": full.truncated,
                "content": full.content,
            });
            if let Ok(line) = serde_json::to_string(&rec) {
                out.push_str(&line);
                out.push('\n');
            }
            count += 1;
            if count >= capped {
                break;
            }
        }
        out
    }

    pub fn read_allowlisted_pdf(&self, requested: &str) -> Option<(String, Vec<u8>)> {
        // Reject any path separator or traversal character first
        if requested.contains('/') || requested.contains('\\') || requested.contains("..") {
            return None;
        }
        let matched = ALLOWLISTED_PDFS
            .iter()
            .find(|(fname, _)| *fname == requested)?;
        let bytes = fs::read(self.pdf_dir.join(matched.0)).ok()?;
        Some((matched.0.to_string(), bytes))
    }

    fn read_doc_excerpt(&self, d: &StoredDoc, terms: &[String], max_chars: usize) -> Option<String> {
        let path = self.source_files.get(d.source_file_idx)?;
        let total = d.byte_end.saturating_sub(d.byte_start);
        let sample_len = total.min(65_536);
        let mut file = File::open(path).ok()?;
        file.seek(SeekFrom::Start(d.byte_start)).ok()?;
        let mut buf = Vec::with_capacity(sample_len as usize);
        file.take(sample_len).read_to_end(&mut buf).ok()?;
        let raw = String::from_utf8_lossy(&buf);
        let clean = clean_preview_text(&raw);
        let lower = clean.to_ascii_lowercase();

        let hit_pos = terms
            .iter()
            .filter_map(|t| lower.find(t.as_str()))
            .min()
            .unwrap_or(0);

        let start = floor_char_boundary(&clean, hit_pos.saturating_sub(90));
        let end = floor_char_boundary(&clean, (start + max_chars).min(clean.len()));
        let mut out = clean[start..end].trim().to_string();
        if start > 0 {
            out = format!("…{out}");
        }
        if end < clean.len() {
            out.push('…');
        }
        Some(out)
    }

    fn push_source_file(&mut self, path: PathBuf) -> usize {
        let idx = self.source_files.len();
        self.source_files.push(path);
        idx
    }

    fn add_doc(&mut self, nd: NewDoc<'_>) {
        let NewDoc {
            collection,
            repo,
            rel_path,
            title,
            source_file_idx,
            byte_start,
            byte_end,
            text_for_index,
            pdf_name,
        } = nd;
        let id = self.docs.len() as u32;
        let byte_len = byte_end.saturating_sub(byte_start);
        let topic = classify_topic(collection, repo, rel_path, title);
        let snippet = make_snippet(text_for_index, 240);
        let hash = blake3::hash(text_for_index.as_bytes()).to_hex();
        let blake3_short = hash[..16].to_string();

        // Build exact-integer per-doc term counts (title + repo + body)
        let mut term_stats: HashMap<String, (u8, u8, u16)> = HashMap::new();
        for t in tokenize_text(title) {
            let e = term_stats.entry(t).or_insert((0, 0, 0));
            e.0 = e.0.saturating_add(1).min(8);
        }
        for t in tokenize_text(repo) {
            let e = term_stats.entry(t).or_insert((0, 0, 0));
            e.1 = e.1.saturating_add(1).min(6);
        }
        for t in tokenize_text(rel_path) {
            let e = term_stats.entry(t).or_insert((0, 0, 0));
            e.1 = e.1.saturating_add(1).min(6);
        }
        for t in tokenize_text(text_for_index) {
            let e = term_stats.entry(t).or_insert((0, 0, 0));
            if e.2 < MAX_BODY_TF_CAP {
                e.2 = e.2.saturating_add(1);
            }
        }

        for (term, (th, rh, bf)) in term_stats {
            self.inverted.entry(term).or_default().push(Posting {
                doc_id: id,
                title_hits: th,
                repo_hits: rh,
                body_tf: bf,
            });
        }

        self.total_bytes = self.total_bytes.saturating_add(byte_len);
        *self.collections.entry(collection.to_string()).or_insert(0) += 1;
        *self.topic_counts.entry(topic.clone()).or_insert(0) += 1;

        self.docs.push(StoredDoc {
            summary: SanctuaryDocSummary {
                id,
                collection: collection.to_string(),
                topic,
                repo: repo.to_string(),
                rel_path: rel_path.to_string(),
                title: title.to_string(),
                byte_len,
                snippet,
                blake3_short,
                pdf_name,
            },
            source_file_idx,
            byte_start,
            byte_end,
        });
    }

    fn ingest_sovereign_library(&mut self, dir: &Path) {
        if !dir.is_dir() {
            return;
        }
        let mut files = Vec::new();
        collect_md_recursive(dir, dir, &mut files, 0);
        files.sort();
        for (rel, path) in files {
            let Ok(raw) = fs::read(&path) else { continue };
            if raw.len() < 128 {
                continue;
            }
            let text = String::from_utf8_lossy(&raw);
            let title = extract_md_heading(&text, &rel);
            let idx = self.push_source_file(path);
            let end = raw.len() as u64;
            self.add_doc(NewDoc {
                collection: "sovereign-library",
                repo: "sovereign-library",
                rel_path: &rel,
                title: &title,
                source_file_idx: idx,
                byte_start: 0,
                byte_end: end,
                text_for_index: &text,
                pdf_name: None,
            });
        }
    }

    fn ingest_downloads_papers(&mut self, dir: &Path) {
        if !dir.is_dir() {
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|n| n.ends_with(".md") && !n.starts_with("00_"))
                        .unwrap_or(false)
            })
            .collect();
        entries.sort();

        for path in entries {
            let Ok(raw) = fs::read(&path) else { continue };
            let rel = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("paper.md")
                .to_string();
            let text = String::from_utf8_lossy(&raw);
            let title = extract_md_heading(&text, &rel);
            let pdf_name = extract_pdf_name_from_meta(&text);
            let idx = self.push_source_file(path);
            let end = raw.len() as u64;
            self.add_doc(NewDoc {
                collection: "downloads-papers",
                repo: "downloads",
                rel_path: &rel,
                title: &title,
                source_file_idx: idx,
                byte_start: 0,
                byte_end: end,
                text_for_index: &text,
                pdf_name,
            });
        }
    }

    fn ingest_canonical_volumes(&mut self, dir: &Path) {
        if !dir.is_dir() {
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut vols: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|n| n.starts_with("CANONICAL_") && n.ends_with(".md"))
                        .unwrap_or(false)
            })
            .collect();
        vols.sort();

        const MARKER: &str = "\n---\n## standardgalactic/";

        for vol_path in vols {
            let Ok(raw_bytes) = fs::read(&vol_path) else {
                continue;
            };
            let raw = String::from_utf8_lossy(&raw_bytes);
            let file_idx = self.push_source_file(vol_path);

            let mut starts: Vec<usize> = Vec::new();
            let mut search_from = 0usize;
            while let Some(pos) = raw[search_from..].find(MARKER) {
                let abs = search_from + pos + 5; // start at "## standardgalactic/..."
                starts.push(abs);
                search_from = abs + 20;
            }

            for i in 0..starts.len() {
                let b_start = starts[i];
                let b_end = if i + 1 < starts.len() {
                    // Back up before "\n---\n"
                    starts[i + 1].saturating_sub(5)
                } else {
                    raw.len()
                };
                if b_end <= b_start {
                    continue;
                }
                let slice = &raw[b_start..b_end];
                let first_line = slice.lines().next().unwrap_or("");
                let full_rel = first_line
                    .strip_prefix("## standardgalactic/")
                    .unwrap_or("unknown")
                    .trim();
                let (repo, rel_in_repo) = match full_rel.split_once('/') {
                    Some((r, p)) => (r, p),
                    None => (full_rel, full_rel),
                };

                let mut title = rel_in_repo.to_string();
                for line in slice.lines().skip(1).take(6) {
                    if let Some(t) = line.strip_prefix("- **Title:** ") {
                        let cleaned = t.trim();
                        if !cleaned.is_empty() {
                            title = cleaned.to_string();
                        }
                        break;
                    }
                }

                // Index first 48 KiB of each manuscript for fast startup & bounded RAM
                let idx_cap = floor_char_boundary(slice, slice.len().min(49_152));
                self.add_doc(NewDoc {
                    collection: "standardgalactic",
                    repo,
                    rel_path: full_rel,
                    title: &title,
                    source_file_idx: file_idx,
                    byte_start: b_start as u64,
                    byte_end: b_end as u64,
                    text_for_index: &slice[..idx_cap],
                    pdf_name: None,
                });
            }
        }
    }
}

fn matches_filters(
    d: &StoredDoc,
    collection_filter: Option<&str>,
    topic_filter: Option<&str>,
) -> bool {
    if let Some(c) = collection_filter {
        let ct = c.trim();
        if !ct.is_empty() && ct != "all" && d.summary.collection != ct {
            return false;
        }
    }
    if let Some(t) = topic_filter {
        let tt = t.trim();
        if !tt.is_empty() && tt != "all" && d.summary.topic != tt {
            return false;
        }
    }
    true
}

fn collect_md_recursive(root: &Path, cur: &Path, out: &mut Vec<(String, PathBuf)>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = fs::read_dir(cur) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        let name = ent.file_name();
        let name_s = name.to_string_lossy();
        if name_s.starts_with('.') || name_s == "node_modules" || name_s == "target" {
            continue;
        }
        if p.is_dir() {
            collect_md_recursive(root, &p, out, depth + 1);
        } else if name_s.ends_with(".md") {
            let rel = p
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_else(|_| name_s.into_owned());
            out.push((rel, p));
        }
    }
}

fn extract_md_heading(text: &str, fallback: &str) -> String {
    for line in text.lines().take(25) {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("# ") {
            let cleaned = h.trim();
            if !cleaned.is_empty() {
                return cleaned.chars().take(180).collect();
            }
        }
    }
    fallback.to_string()
}

fn extract_pdf_name_from_meta(text: &str) -> Option<String> {
    for line in text.lines().take(12) {
        if let Some(rest) = line.strip_prefix("- **Offline PDF Viewer:** `/api/library/pdf/") {
            if let Some(fname) = rest.strip_suffix('`') {
                return Some(fname.to_string());
            }
        }
    }
    None
}

fn classify_topic(collection: &str, repo: &str, rel_path: &str, title: &str) -> String {
    if collection == "sovereign-library" {
        return "pillars".to_string();
    }
    let hay = format!(
        "{} {} {}",
        repo.to_ascii_lowercase(),
        rel_path.to_ascii_lowercase(),
        title.to_ascii_lowercase()
    );
    if contains_any(
        &hay,
        &[
            "ultralove",
            "ontology",
            "epistemolog",
            "admissibility",
            "guardrail",
            "repair",
            "evidentiary",
            "philosophy",
            "compassion",
            "respect",
            "ethic",
            "standing",
            "diatribe",
            "blind-spots",
        ],
    ) {
        return "pillars".to_string();
    }
    if contains_any(
        &hay,
        &[
            "cosmolog",
            "dark-energy",
            "dark energy",
            "gamma-waves",
            "antigravity",
            "rsvp",
            "plenum",
            "quantum",
            "caldera",
            "photonics",
            "thermodynamic",
            "reluctance",
            "spinodal",
            "entropic",
            "physics",
        ],
    ) {
        return "physics".to_string();
    }
    if contains_any(
        &hay,
        &[
            "spherepop",
            "ayeos",
            "ayefire",
            "exact-integer",
            "rust",
            "cryptograph",
            "execution_is_not_validation",
            "execution is not validation",
            "portable-warrant",
            "verification",
            "bootloader",
            "marine algorithm",
            "kernel",
            "computation",
            "circuit",
            "tartan",
            "hydra",
            "centerfuge",
            "userland",
            "userspace",
        ],
    ) {
        return "systems".to_string();
    }
    if contains_any(
        &hay,
        &[
            "active-inference",
            "geodesic",
            "cognition",
            "memnet",
            "mem8",
            "mem-8",
            "memory",
            "alignment",
            "intelligence",
            "interpretability",
            "clio",
            "neocortex",
            "mind",
            "consciousness",
            "yarncrawler",
        ],
    ) {
        return "mind".to_string();
    }
    "craft".to_string()
}

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

fn clean_preview_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(4096));
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            continue;
        }
        if t.starts_with('%')
            || t.starts_with("\\usepackage")
            || t.starts_with("\\documentclass")
            || t.starts_with("\\newcommand")
            || t.starts_with("\\renewcommand")
            || t.starts_with("\\definecolor")
            || t.starts_with("\\geometry")
            || t.starts_with("- **Bytes:**")
            || t.starts_with("- **Source")
            || t.starts_with("## standardgalactic/")
            || t == "---"
        {
            continue;
        }
        if !t.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(t);
            if out.len() >= 4096 {
                break;
            }
        }
    }
    out
}

fn make_snippet(raw: &str, max_chars: usize) -> String {
    let clean = clean_preview_text(raw);
    let end = floor_char_boundary(&clean, max_chars.min(clean.len()));
    let mut s = clean[..end].trim().to_string();
    if end < clean.len() {
        s.push('…');
    }
    s
}

fn floor_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn is_stopword(w: &str) -> bool {
    matches!(
        w,
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "that"
            | "this"
            | "into"
            | "are"
            | "was"
            | "were"
            | "have"
            | "has"
            | "not"
            | "but"
            | "all"
            | "can"
            | "its"
            | "our"
            | "their"
            | "which"
            | "when"
            | "where"
            | "while"
            | "also"
            | "than"
            | "then"
            | "only"
            | "such"
            | "each"
            | "more"
            | "some"
            | "they"
            | "them"
            | "what"
            | "does"
            | "been"
            | "will"
            | "begin"
            | "end"
            | "section"
            | "subsection"
            | "textbf"
            | "textit"
            | "emph"
            | "item"
            | "frac"
            | "left"
            | "right"
            | "text"
            | "latex"
            | "document"
            | "usepackage"
            | "label"
            | "ref"
            | "cite"
    )
}

fn tokenize_text(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            if cur.len() >= 3 && cur.len() <= 32 && !is_stopword(&cur) {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() >= 3 && cur.len() <= 32 && !is_stopword(&cur) {
        out.push(cur);
    }
    out
}

fn tokenize_query(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tok in tokenize_text(query) {
        if seen.insert(tok.clone()) {
            out.push(tok);
            if out.len() >= 12 {
                break;
            }
        }
    }
    out
}

fn url_encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Calm, zero-purple, zero-CDN standalone offline reading room page for `/read/:id`.
pub fn render_reader_html(full: &SanctuaryDocFull) -> String {
    let d = &full.doc;
    let pdf_btn = match &d.pdf_name {
        Some(fname) => format!(
            r#"<a class="btn primary" href="/api/library/pdf/{}" target="_blank">Open Offline PDF ({})</a>"#,
            url_encode_segment(fname),
            escape_html(fname)
        ),
        None => String::new(),
    };
    let trunc_banner = if full.truncated {
        r#"<div class="notice">Showing first 512 KiB of this manuscript for fast offline reading.</div>"#
    } else {
        ""
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>{title} — AyeFire Sanctuary Library</title>
<style>
:root{{
  --bg:#05060a;--panel:#0d101b;--elev:#131a2b;--bdr:#243146;--bdr-hi:#36506e;
  --ink:#e9f1f7;--muted:#93a6bf;--mint:#4ecdc4;--cyan:#00f0ff;--ok:#2ecc71;--gold:#e8b05c;
  color-scheme:dark;
}}
*{{box-sizing:border-box;}}
body{{
  margin:0;background:var(--bg);color:var(--ink);
  font-family:ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,sans-serif;
  line-height:1.65;padding:20px clamp(14px,4vw,40px) 60px;
}}
.wrap{{max-inline-size:84ch;margin-inline:auto;display:grid;gap:16px;}}
header{{
  background:var(--elev);border:1px solid var(--mint);border-radius:12px;
  padding:16px 20px;display:grid;gap:10px;
}}
h1{{margin:0;color:var(--cyan);font-size:clamp(1.15rem,1rem + 0.8vw,1.55rem);line-height:1.3;}}
.meta{{display:flex;flex-wrap:wrap;gap:8px;font-size:0.82rem;color:var(--muted);}}
.pill{{
  display:inline-flex;align-items:center;padding:3px 10px;border-radius:999px;
  border:1px solid var(--bdr-hi);background:var(--panel);color:var(--ink);font-size:0.78rem;
}}
.pill.mint{{border-color:var(--mint);color:var(--mint);}}
.pill.ok{{border-color:var(--ok);color:var(--ok);}}
.pill.gold{{border-color:var(--gold);color:var(--gold);}}
.actions{{display:flex;flex-wrap:wrap;gap:8px;}}
.btn{{
  font:inherit;font-size:0.84rem;padding:7px 14px;border-radius:8px;text-decoration:none;
  border:1px solid var(--bdr-hi);background:var(--panel);color:var(--ink);display:inline-flex;align-items:center;
}}
.btn.primary{{background:var(--mint);border-color:var(--mint);color:#05060a;font-weight:700;}}
.btn:hover{{border-color:var(--cyan);color:var(--cyan);}}
.btn.primary:hover{{background:var(--cyan);border-color:var(--cyan);color:#05060a;}}
.notice{{
  background:var(--elev);border:1px solid var(--gold);color:var(--gold);
  border-radius:8px;padding:10px 14px;font-size:0.85rem;
}}
article{{
  background:var(--panel);border:1px solid var(--bdr);border-radius:12px;
  padding:22px clamp(16px,3vw,30px);
}}
pre.manuscript{{
  margin:0;white-space:pre-wrap;overflow-wrap:anywhere;
  font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;
  font-size:0.9rem;line-height:1.62;color:var(--ink);
}}
footer{{text-align:center;font-size:0.8rem;color:var(--muted);padding-top:8px;}}
</style>
</head>
<body>
<main class="wrap">
  <header>
    <div class="actions">
      <a class="btn" href="/">← Back to AyeFire Sanctuary</a>
      <a class="btn" href="/api/library/doc/{id}">JSON View</a>
      {pdf_btn}
    </div>
    <h1>{title}</h1>
    <div class="meta">
      <span class="pill mint">{collection}</span>
      <span class="pill ok">topic: {topic}</span>
      <span class="pill gold">{repo}</span>
      <span class="pill">{bytes} bytes</span>
      <span class="pill">BLAKE3: {blake3}</span>
      <span class="pill ok">100% Offline · Zero Tracking</span>
    </div>
    <div class="meta"><span>Source: <code>{rel_path}</code></span></div>
  </header>
  {trunc_banner}
  <article>
    <pre class="manuscript">{content}</pre>
  </article>
  <footer>AyeFire Sanctuary Internet · Education · Understanding · Love · Respect · Space · &#123;&lt;3,&lt;3,&lt;3&#125;+1</footer>
</main>
</body>
</html>"##,
        id = d.id,
        title = escape_html(&d.title),
        collection = escape_html(&d.collection),
        topic = escape_html(&d.topic),
        repo = escape_html(&d.repo),
        bytes = d.byte_len,
        blake3 = escape_html(&d.blake3_short),
        rel_path = escape_html(&d.rel_path),
        pdf_btn = pdf_btn,
        trunc_banner = trunc_banner,
        content = escape_html(&full.content),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_integer_search_ranks_title_and_body_deterministically() {
        let tmp = std::env::temp_dir().join(format!(
            "ayefire-sanctuary-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let sov = tmp.join("sov");
        let dl = tmp.join("dl");
        let canon = tmp.join("canon");
        fs::create_dir_all(&sov).unwrap();
        fs::create_dir_all(&dl).unwrap();
        fs::create_dir_all(&canon).unwrap();

        fs::write(
            sov.join("pillars.md"),
            "# Six Pillars of Holding\n\nCompassion, Understanding, Moderation, Love, Respect, and Space.\n".repeat(4),
        )
        .unwrap();
        fs::write(
            canon.join("CANONICAL_S_01.md"),
            "# Test Volume\n\n---\n## standardgalactic/spherepop/spherepop-os.tex\n- **Title:** Spherepop OS Deterministic Kernel\n- **Bytes:** 1200\n\n```latex\nSpherepop is a deterministic event-sourced semantic operating system with realizability constraints.\n```\n",
        )
        .unwrap();

        let lib = SanctuaryLibrary::load_from_dirs(&sov, &dl, &canon, &tmp);
        assert_eq!(lib.status().total_docs, 2);
        assert!(lib.status().offline_only);
        assert!(lib.status().no_tracking);
        assert!(lib.status().zero_float_enforced);

        let hits = lib.search("spherepop deterministic kernel", None, None, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc.repo, "spherepop");
        assert!(hits[0].score > 0);

        let full = lib.read_doc(hits[0].doc.id).unwrap();
        assert!(full.content.contains("event-sourced semantic operating system"));

        // Verify strict PDF allowlist blocks path traversal
        assert!(lib.read_allowlisted_pdf("../etc/passwd").is_none());
        assert!(lib.read_allowlisted_pdf("unlisted.pdf").is_none());

        let _ = fs::remove_dir_all(&tmp);
    }
}
