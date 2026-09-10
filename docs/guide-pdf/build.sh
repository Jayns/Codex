#!/usr/bin/env bash
# Build the portable-guide PDF(s) from the Markdown sources under docs/.
#
#   pandoc (Markdown -> self-contained HTML)
#     -> headless Chrome (HTML -> PDF)
#     -> footer.py (stamp title + page numbers)
#
# Usage:  bash docs/guide-pdf/build.sh [windows|macos|all]   (default: all)
#
# Requirements: pandoc, a Chrome/Chromium/Edge binary, python with `pymupdf`.
# Output: output/pdf/*.pdf  (git-ignored)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
cd "$repo"

target="${1:-all}"
out_dir="output/pdf"

# locate a Chrome/Chromium binary once
chrome=""
for c in \
  "${CHROME_BIN:-}" \
  "/c/Program Files/Google/Chrome/Application/chrome.exe" \
  "/c/Program Files (x86)/Google/Chrome/Application/chrome.exe" \
  "/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" \
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  "$(command -v google-chrome 2>/dev/null || true)" \
  "$(command -v chromium 2>/dev/null || true)" \
  "$(command -v chrome 2>/dev/null || true)"; do
  if [ -n "${c:-}" ] && [ -x "$c" ]; then chrome="$c"; break; fi
done
if [ -z "$chrome" ]; then
  echo "error: no Chrome/Chromium found. Set CHROME_BIN to a browser binary." >&2
  exit 1
fi

build_one() {
  local src="$1" title="$2" cover="$3" out_pdf="$out_dir/$4"

  local tmp tmpw
  tmp="$(mktemp -d)"
  # pandoc / Chrome / python here may be native Windows binaries and need a
  # Windows-style path; fall back to the raw path elsewhere.
  tmpw="$(cygpath -m "$tmp" 2>/dev/null || printf '%s' "$tmp")"

  pandoc "$src" \
    -f gfm -t html5 --standalone --section-divs \
    --embed-resources --resource-path=docs \
    --metadata title="$title" \
    --include-before-body "$cover" \
    --css "$here/style.css" \
    -o "$tmpw/guide.html"

  "$chrome" --headless=new --disable-gpu --no-pdf-header-footer \
    --print-to-pdf="$tmpw/guide_raw.pdf" "file:///$tmpw/guide.html"

  mkdir -p "$out_dir"
  python "$here/footer.py" "$tmpw/guide_raw.pdf" "$out_pdf" "$title"
  rm -rf "$tmp"
  echo "built $out_pdf"
}

case "$target" in
  windows|win)
    build_one "docs/ChatGPT便携版使用说明.md" \
      "ChatGPT 便携版使用说明（Windows）" \
      "$here/cover-windows.html" \
      "ChatGPT便携版使用说明.pdf" ;;
  macos|mac)
    build_one "docs/ChatGPT便携版使用说明-macOS.md" \
      "ChatGPT 便携版使用说明（macOS）" \
      "$here/cover-macos.html" \
      "ChatGPT便携版使用说明-macOS.pdf" ;;
  all)
    build_one "docs/ChatGPT便携版使用说明.md" \
      "ChatGPT 便携版使用说明（Windows）" \
      "$here/cover-windows.html" \
      "ChatGPT便携版使用说明.pdf"
    build_one "docs/ChatGPT便携版使用说明-macOS.md" \
      "ChatGPT 便携版使用说明（macOS）" \
      "$here/cover-macos.html" \
      "ChatGPT便携版使用说明-macOS.pdf" ;;
  *)
    echo "usage: bash docs/guide-pdf/build.sh [windows|macos|all]" >&2
    exit 2 ;;
esac
