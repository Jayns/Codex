#!/usr/bin/env bash
# Build the portable-guide PDF from docs/ChatGPT便携版使用说明.md
#
#   pandoc (Markdown -> self-contained HTML)
#     -> headless Chrome (HTML -> PDF)
#     -> footer.py (stamp title + page numbers)
#
# Requirements: pandoc, a Chrome/Chromium binary, python with `pymupdf`.
# Output: output/pdf/ChatGPT便携版使用说明.pdf  (git-ignored)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
cd "$repo"

src="docs/ChatGPT便携版使用说明.md"
title="ChatGPT 便携版使用说明（Windows）"
out_dir="output/pdf"
out_pdf="$out_dir/ChatGPT便携版使用说明.pdf"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
# pandoc / Chrome / python here are native Windows binaries and need a
# Windows-style path; fall back to the raw path off Windows.
tmpw="$(cygpath -m "$tmp" 2>/dev/null || printf '%s' "$tmp")"

# 1. Markdown -> HTML (images inlined as data URIs)
pandoc "$src" \
  -f gfm -t html5 --standalone --section-divs \
  --embed-resources --resource-path=docs \
  --metadata title="$title" \
  --include-before-body "$here/cover.html" \
  --css "$here/style.css" \
  -o "$tmpw/guide.html"

# 2. locate a Chrome/Chromium binary
chrome=""
for c in \
  "${CHROME_BIN:-}" \
  "/c/Program Files/Google/Chrome/Application/chrome.exe" \
  "/c/Program Files (x86)/Google/Chrome/Application/chrome.exe" \
  "/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" \
  "$(command -v google-chrome 2>/dev/null || true)" \
  "$(command -v chromium 2>/dev/null || true)" \
  "$(command -v chrome 2>/dev/null || true)"; do
  if [ -n "${c:-}" ] && [ -x "$c" ]; then chrome="$c"; break; fi
done
if [ -z "$chrome" ]; then
  echo "error: no Chrome/Chromium found. Set CHROME_BIN to a browser binary." >&2
  exit 1
fi

# 3. HTML -> PDF
"$chrome" --headless=new --disable-gpu --no-pdf-header-footer \
  --print-to-pdf="$tmpw/guide_raw.pdf" "file:///$tmpw/guide.html"

# 4. stamp footer + page numbers
mkdir -p "$out_dir"
python "$here/footer.py" "$tmpw/guide_raw.pdf" "$out_pdf" "$title"

echo "built $out_pdf"
