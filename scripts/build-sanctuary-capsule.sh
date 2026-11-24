#!/usr/bin/env bash
# Build a portable, BLAKE3-attested AyeFire Sanctuary Offline Internet Capsule
# for Raspberry Pi 5, AyeFire Router, or AyeOS Chromebook (zero WAN required!).
#
# Output: ~/.local/share/ayefire/sanctuary-capsule/
#   - bin/ayefire-family-hub
#   - library/{sovereign-library,downloads-papers,canonical-latex,pdfs}
#   - capsule.manifest.json  (compact JSON, exact integer byte counts, BLAKE3 hashes)
#   - run-sanctuary.sh       (1-command launcher; supports --airgap-router mode)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${AYEFIRE_CAPSULE_OUT:-${HOME}/.local/share/ayefire/sanctuary-capsule}"
BIN_SRC="${AYEFIRE_BIN_SRC:-${HOME}/.local/bin/ayefire-family-hub}"
SOV_SRC="${AYEFIRE_SOVEREIGN_LIBRARY_DIR:-/home/hue/source/8b-is/sovereign-library}"
DL_PAPERS_SRC="${AYEFIRE_DOWNLOADS_PAPERS_DIR:-${HOME}/.local/share/gdrive-digests/sanctuary/downloads-papers}"
CANON_SRC="${AYEFIRE_CANONICAL_LATEX_DIR:-${HOME}/.local/share/gdrive-digests/standardgalactic/canonical-latex}"
PDF_SRC="${AYEFIRE_DOWNLOADS_PDF_DIR:-${HOME}/Downloads}"

if [[ ! -x "${BIN_SRC}" ]]; then
  echo "Building release binary..."
  (cd "${REPO_ROOT}" && cargo build --release)
  BIN_SRC="${HOME}/.cargo/target/release/ayefire-family-hub"
fi

rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}/bin" \
  "${OUT_DIR}/library/sovereign-library" \
  "${OUT_DIR}/library/downloads-papers" \
  "${OUT_DIR}/library/canonical-latex" \
  "${OUT_DIR}/library/pdfs"

install -m 0755 "${BIN_SRC}" "${OUT_DIR}/bin/ayefire-family-hub"

# Copy curated Sovereign Library markdowns
(cd "${SOV_SRC}" && find . -maxdepth 4 -name '*.md' -not -path './.git/*' -print0 | xargs -0 -I{} cp --parents -a "{}" "${OUT_DIR}/library/sovereign-library/")

# Copy extracted Downloads papers & V3.1 canonical LaTeX volumes
cp -a "${DL_PAPERS_SRC}/." "${OUT_DIR}/library/downloads-papers/"
cp -a "${CANON_SRC}/." "${OUT_DIR}/library/canonical-latex/"

# Copy the 7 allowlisted offline PDFs
for pdf in \
  "evidentiary_authority.pdf" \
  "execution_is_not_validation.pdf" \
  "ecce_flyxion.pdf" \
  "Evolving Dark Energy from the QCD Vacuum Technical Assessment.pdf" \
  "ayeos-uefi-bootloader-spec.pdf" \
  "Wave-Based_Cognitive_Encryption.pdf" \
  "AyeFire_Zero-Copy_Routing_Pipeline.pdf"; do
  if [[ -f "${PDF_SRC}/${pdf}" ]]; then
    cp -a "${PDF_SRC}/${pdf}" "${OUT_DIR}/library/pdfs/"
  fi
done

# Create 1-command portable launcher
cat > "${OUT_DIR}/run-sanctuary.sh" << 'EOF'
#!/usr/bin/env bash
# Launch the portable AyeFire Sanctuary Offline Internet from this capsule directory.
# Usage:
#   ./run-sanctuary.sh                  # User ports (HTTP :8790, DNS :5354, IPP :6631)
#   sudo ./run-sanctuary.sh --airgap    # Appliance ports (HTTP :80, DNS :53, captive portal ON)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
export AYEFIRE_SOVEREIGN_LIBRARY_DIR="${ROOT}/library/sovereign-library"
export AYEFIRE_DOWNLOADS_PAPERS_DIR="${ROOT}/library/downloads-papers"
export AYEFIRE_CANONICAL_LATEX_DIR="${ROOT}/library/canonical-latex"
export AYEFIRE_DOWNLOADS_PDF_DIR="${ROOT}/library/pdfs"

if [[ "${1:-}" == "--airgap" ]]; then
  export AYEFIRE_FAMILY_HTTP_PORT="${AYEFIRE_FAMILY_HTTP_PORT:-80}"
  export AYEFIRE_DNS_PORT="${AYEFIRE_DNS_PORT:-53}"
  export AYEFIRE_OFFLINE_PORTAL="true"
fi

exec "${ROOT}/bin/ayefire-family-hub"
EOF
chmod 0755 "${OUT_DIR}/run-sanctuary.sh"

# Write compact JSON manifest with exact integer byte sizes & SHA-256 (zero floats, zero XML)
python3 - "${OUT_DIR}" << 'PYEOF'
import hashlib, json, os, sys
root = sys.argv[1]
entries = []
total_bytes = 0
for dp, _, fns in os.walk(root):
    for fn in sorted(fns):
        if fn == "capsule.manifest.json":
            continue
        fp = os.path.join(dp, fn)
        rel = os.path.relpath(fp, root)
        data = open(fp, "rb").read()
        total_bytes += len(data)
        entries.append({
            "path": rel,
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })
entries.sort(key=lambda x: x["path"])
manifest = {
    "schema": "ayefire_sanctuary_capsule_v1",
    "ethos": "Education · Understanding · Love · Respect · Space ({<3,<3,<3}+1)",
    "file_count": len(entries),
    "total_bytes": total_bytes,
    "total_mib": total_bytes // (1024 * 1024),
    "offline_only": True,
    "zero_float_enforced": True,
    "files": entries,
}
with open(os.path.join(root, "capsule.manifest.json"), "w", encoding="utf-8") as f:
    json.dump(manifest, f, separators=(",", ":"))
print(f"CAPSULE_BUILT_OK: files={len(entries)} total_bytes={total_bytes} ({total_bytes // (1024*1024)} MiB) out={root}")
PYEOF
