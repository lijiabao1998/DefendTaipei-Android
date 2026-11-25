#!/usr/bin/env python3
"""Bake the shop-sign atlases the original game draws on <canvas> with fillText.

Ports the sign tables of defendtaipei `src/map/textures.ts` (SIGN, b2sign, PARODY) and
`src/map/walkup.ts` (vertical signs VS / VSC). Text needs a CJK font, so instead of shipping a
full font to the phone the signs are rendered once here (Noto Sans CJK, SIL OFL 1.1) and the
PNGs are embedded in the game binary.

    python3 tools/generate_signs.py   # writes assets/textures/signs.png, vsigns.png
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "textures"
FONT = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"  # TC face is index 3 or 4

def font(size):
    for idx in range(0, 10):
        try:
            f = ImageFont.truetype(FONT, size, index=idx)
            if "TC" in " ".join(f.getname()):
                return f
        except OSError:
            break
    return ImageFont.truetype(FONT, size)

# (text, bg, fg) — order is the atlas index used by game/src/map/textures.rs (SIGN_*)
SIGNS = [
    # PARODY shop signs (shopSignTex), colours cycle like the original
    *[(t, ["#a8221e", "#20344f", "#c9971c", "#6a2a8a", "#2a6a3a"][i % 5], "#fff6d0")
      for i, t in enumerate(["阿嬤小吃", "金萬華當舖", "老牌麵線", "西門電玩", "幸福理容",
                             "大眾五金", "青草茶", "鐘錶行", "刺青", "金紙香舖"])],
    # SIGN (signTex)
    ("7-12", "#e8eef0", "#d23a1e"),
    ("康是福藥局", "#1d8a5a", "#ffffff"),
    ("派出所", "#1d3f8a", "#ffffff"),
    # b2sign
    ("金來雜貨店", "#b8241c", "#fff2a0"),
    ("晨光早餐", "#f2a81c", "#7a1a10"),
    ("檳榔・香菸", "#111111", "#33ff66"),
    ("全民福利超市", "#1a4aa8", "#ffffff"),
    ("永興五金行", "#f0c020", "#1a1a1a"),
]
COLS = 4

def signs():
    rows = (len(SIGNS) + COLS - 1) // COLS
    im = Image.new("RGB", (256 * COLS, 64 * rows), "#000000")
    d = ImageDraw.Draw(im)
    for i, (t, bg, fg) in enumerate(SIGNS):
        x, y = (i % COLS) * 256, (i // COLS) * 64
        d.rectangle([x, y, x + 255, y + 63], fill=bg)
        if t == "7-12":
            d.rectangle([x, y, x + 255, y + 9], fill="#f08a1c")
            d.rectangle([x, y + 54, x + 255, y + 63], fill="#1c9a4a")
        size = 40 if i >= 10 else 38
        f = font(size)
        while d.textlength(t, font=f) > 240 and size > 16:
            size -= 2
            f = font(size)
        d.text((x + 128, y + 34), t, font=f, fill=fg, anchor="mm", stroke_width=1, stroke_fill=fg)
    im.save(OUT / "signs.png", optimize=True)

VS = ["一家川菜", "老王麵店", "美美理髮", "金福當舖", "阿嬤小吃", "永興五金", "大眾診所", "好運彩券"]
VSC = [("#c8231c", "#ffffff"), ("#f2efe4", "#b01e18"), ("#1d4f8a", "#ffffff"), ("#f0c419", "#a01810")]

def vsigns():
    im = Image.new("RGB", (64 * len(VS), 256), "#000000")
    d = ImageDraw.Draw(im)
    f = font(44)
    for i, s in enumerate(VS):
        bg, fg = VSC[i % len(VSC)]
        d.rectangle([i * 64, 0, i * 64 + 63, 255], fill=bg)
        d.rectangle([i * 64 + 4, 4, i * 64 + 59, 251], outline=fg, width=3)
        for j, ch in enumerate(s):
            d.text((i * 64 + 32, 52 + j * 58), ch, font=f, fill=fg, anchor="mm", stroke_width=1, stroke_fill=fg)
    im.save(OUT / "vsigns.png", optimize=True)

if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    signs()
    vsigns()
    print("wrote", OUT / "signs.png", OUT / "vsigns.png")
