#!/usr/bin/env python3
"""Rebuild the Noto Sans CJK TC subset from displayed strings.

Scans every Rust source file for string literals (same rules as
`displayed_chars` in `game/src/i18n.rs`) and always keeps °←↑→↓▶.
Writes `assets/fonts/DefendTaipeiUI.otf` and
`assets/fonts/NotoSansTC-Subset.otf`. Re-run after story or UI copy changes.

Requires fontTools (`pip install fonttools`). The OFL source font is
downloaded on first use to the path in `NOTO_SANS_TC` or `/tmp/fonts`.
"""

from __future__ import annotations

import os
import json
import urllib.request
from pathlib import Path

EXTRA = "°←↑→↓▶"
SKIP_DIRS = {"target", ".git"}
SOURCE_URL = (
    "https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF/"
    "TraditionalChinese/NotoSansCJKtc-Regular.otf"
)


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    chars = set(EXTRA)
    for path in rust_files(root):
        scan_strings(path.read_text(encoding="utf-8"), chars)
    # The survivor map prints real street labels from the bundled OSM data.
    world = json.loads((root / "assets/data/wanhua.json").read_text(encoding="utf-8"))
    for road in world["roads"]:
        chars.update(road.get("n", ""))
    text_path = Path("/tmp/fonts/chars.txt")
    text_path.parent.mkdir(parents=True, exist_ok=True)
    text_path.write_text("".join(sorted(chars)), encoding="utf-8")

    source = Path(os.environ.get("NOTO_SANS_TC", "/tmp/fonts/NotoSansCJKtc-Regular.otf"))
    if not source.exists():
        print(f"downloading {SOURCE_URL}")
        urllib.request.urlretrieve(SOURCE_URL, source)

    from fontTools.subset import main as subset_main
    from fontTools.ttLib import TTFont

    out = Path("/tmp/fonts/DefendTaipeiUI.otf")
    subset_main(
        [
            str(source),
            f"--text-file={text_path}",
            f"--output-file={out}",
            "--layout-features=kern,liga",
            "--recommended-glyphs",
            "--notdef-glyph",
            "--notdef-outline",
            "--legacy-cmap",
        ]
    )
    font = TTFont(out)
    for rec in font["name"].names:
        if rec.nameID in (1, 4, 6, 16):
            rec.string = "Defend Taipei UI"
    font.save(out)
    data = out.read_bytes()
    for dest in (
        root / "assets/fonts/DefendTaipeiUI.otf",
        root / "assets/fonts/NotoSansTC-Subset.otf",
    ):
        dest.write_bytes(data)
        print(f"wrote {dest} ({len(data)} bytes, {len(chars)} chars)")


def rust_files(root: Path):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [
            name
            for name in dirnames
            if name not in SKIP_DIRS and not name.startswith(".")
        ]
        for name in filenames:
            if name.endswith(".rs"):
                yield Path(dirpath) / name


def scan_strings(src: str, out: set[str]) -> None:
    chars = list(src)
    i = 0
    while i < len(chars):
        if chars[i] == "/" and i + 1 < len(chars) and chars[i + 1] == "/":
            i += 2
            while i < len(chars) and chars[i] != "\n":
                i += 1
            continue
        if chars[i] == "/" and i + 1 < len(chars) and chars[i + 1] == "*":
            i += 2
            while i + 1 < len(chars) and not (chars[i] == "*" and chars[i + 1] == "/"):
                i += 1
            i = min(i + 2, len(chars))
            continue
        if chars[i] == "r":
            body = raw_string(chars, i)
            if body is not None:
                out.update(body[0])
                i = body[1]
                continue
        if chars[i] == '"':
            body = cooked_string(chars, i)
            if body is not None:
                out.update(body[0])
                i = body[1]
                continue
        i += 1


def raw_string(chars: list[str], start: int):
    i = start + 1
    hashes = 0
    while i < len(chars) and chars[i] == "#":
        hashes += 1
        i += 1
    if i >= len(chars) or chars[i] != '"':
        return None
    i += 1
    body_start = i
    while i < len(chars):
        if chars[i] == '"' and chars[i + 1 : i + 1 + hashes] == ["#"] * hashes:
            return (chars[body_start:i], i + 1 + hashes)
        i += 1
    return None


def cooked_string(chars: list[str], start: int):
    i = start + 1
    body: list[str] = []
    while i < len(chars):
        ch = chars[i]
        if ch == '"':
            return (body, i + 1)
        if ch == "\\":
            i += 1
            if i >= len(chars):
                return None
            escaped = chars[i]
            mapping = {"n": "\n", "r": "\r", "t": "\t", "0": "\0"}
            if escaped in mapping:
                body.append(mapping[escaped])
            elif escaped in "\\'\"":
                body.append(escaped)
            elif escaped == "u" and i + 1 < len(chars) and chars[i + 1] == "{":
                i += 2
                hex_start = i
                while i < len(chars) and chars[i] in "0123456789abcdefABCDEF":
                    i += 1
                if i >= len(chars) or chars[i] != "}":
                    return None
                body.append(chr(int("".join(chars[hex_start:i]), 16)))
            else:
                body.append(escaped)
            i += 1
            continue
        body.append(ch)
        i += 1
    return None


if __name__ == "__main__":
    main()
