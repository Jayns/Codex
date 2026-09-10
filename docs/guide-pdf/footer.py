"""Stamp a footer (doc title + page number) onto every page except the cover.

Usage: python footer.py <in.pdf> <out.pdf> [title]
"""

import sys

import pymupdf

src = sys.argv[1]
dst = sys.argv[2]
title = sys.argv[3] if len(sys.argv) > 3 else "ChatGPT 便携版使用说明（Windows）"

doc = pymupdf.open(src)
n = len(doc)
font = pymupdf.Font("china-s")
GRAY = (0.55, 0.58, 0.62)

for i, page in enumerate(doc):
    if i == 0:
        continue  # skip cover page
    r = page.rect
    y = r.height - 24
    page.draw_line(
        pymupdf.Point(48, y - 10),
        pymupdf.Point(r.width - 48, y - 10),
        color=(0.85, 0.88, 0.92),
        width=0.6,
    )
    tw = pymupdf.TextWriter(r, color=GRAY)
    tw.append(pymupdf.Point(48, y), title, font=font, fontsize=7.5)
    label = f"{i + 1} / {n}"
    w = font.text_length(label, fontsize=7.5)
    tw.append(pymupdf.Point(r.width - 48 - w, y), label, font=font, fontsize=7.5)
    tw.write_text(page)

doc.subset_fonts()
doc.save(dst, garbage=4, deflate=True)
print(f"saved {dst} ({n} pages)")
