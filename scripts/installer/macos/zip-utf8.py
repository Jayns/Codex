#!/usr/bin/env python3
"""Zips a directory with the UTF-8 filename flag (general purpose bit 11) set
on every entry.

Neither macOS's built-in `zip` (Apple's Info-ZIP 3.0 build strips the
`-UN=UTF8` option despite documenting it) nor `ditto -c -k` set this flag,
so a zip containing non-ASCII names round-trips fine between two macOS
machines but fails to open elsewhere: strict zip readers that don't set the
flag fall back to decoding names as CP437, garbling anything non-ASCII, and
some (e.g. macOS's own Archive Utility, when the corruption is bad enough)
reject the archive outright as an unrecognized format. See
scripts/installer/macos/package-portable.sh, whose portable folder always
contains at least one Chinese filename (使用说明.txt).

Usage: zip-utf8.py <source-dir> <output.zip>

Zips the *contents* of source-dir directly at the archive root (i.e. no
extra wrapping folder beyond what source-dir's own basename implies) if
source-dir's parent is passed; to keep a top-level folder in the archive
(matching `ditto -c -k --keepParent` / `zip -r`), pass the folder itself
as source-dir — that folder's name becomes the archive's top-level entry.
"""

import os
import sys
import time
import zipfile

UTF8_FLAG = 0x800


def add_directory(zf: zipfile.ZipFile, src_root: str, arc_root: str) -> None:
    for dirpath, dirnames, filenames in os.walk(src_root):
        rel_dir = os.path.relpath(dirpath, os.path.dirname(src_root))
        arcname_dir = os.path.join(arc_root, rel_dir) if arc_root else rel_dir
        if not dirnames and not filenames:
            info = zipfile.ZipInfo(arcname_dir.replace(os.sep, "/") + "/")
            info.flag_bits |= UTF8_FLAG
            info.date_time = time.localtime(os.path.getmtime(dirpath))[:6]
            zf.writestr(info, b"")
        for name in filenames:
            full = os.path.join(dirpath, name)
            rel = os.path.join(rel_dir, name)
            arcname = (os.path.join(arc_root, rel) if arc_root else rel).replace(os.sep, "/")
            info = zipfile.ZipInfo(arcname)
            info.flag_bits |= UTF8_FLAG
            info.date_time = time.localtime(os.path.getmtime(full))[:6]
            info.compress_type = zipfile.ZIP_DEFLATED
            # Preserve Unix permissions (executable bits) the same way
            # `zip`/`ditto` do, via the high 16 bits of external_attr.
            info.external_attr = (os.lstat(full).st_mode & 0xFFFF) << 16
            with open(full, "rb") as handle:
                zf.writestr(info, handle.read())


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <source-dir> <output.zip>", file=sys.stderr)
        return 2
    source_dir, output_zip = sys.argv[1], sys.argv[2]
    if not os.path.isdir(source_dir):
        print(f"error: not a directory: {source_dir}", file=sys.stderr)
        return 1
    source_dir = source_dir.rstrip(os.sep)
    top_level = os.path.basename(source_dir)
    with zipfile.ZipFile(output_zip, "w", zipfile.ZIP_DEFLATED) as zf:
        add_directory(zf, source_dir, arc_root="")
    print(f"wrote {output_zip} ({os.path.getsize(output_zip)} bytes, top-level: {top_level}/)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
