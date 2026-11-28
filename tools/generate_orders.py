#!/usr/bin/env python3
"""Bake the occupier's order-banner atlas (Web `src/systems/reclaim.ts` `ordersAtlas`).

1024 x 512: four vertical red banners (128 px each) with yellow characters on the
left half, four horizontal cross-street notices (512 x 128) with white text on the
right half. All the orders are invented, as in the Web build. Text needs a CJK
font, so the atlas is rendered once here (Noto Sans CJK, SIL OFL 1.1) and the PNG
is embedded in the game binary.

    python3 tools/generate_orders.py   # writes assets/textures/orders.png
"""
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "textures"
FONT = "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc"

ORDERS_V = ["宵禁後禁止外出", "服從管制維護秩序", "舉報可疑人員", "軍事管制區"]
ORDERS_H = ["軍事管制區　禁止聚集", "二十時起全面宵禁", "交出武器　既往不咎", "違者依戰時法令處置"]


def font(size):
    for idx in range(0, 10):
        try:
            f = ImageFont.truetype(FONT, size, index=idx)
            if "TC" in " ".join(f.getname()):
                return f
        except OSError:
            break
    return ImageFont.truetype(FONT, size)


def cloth(d, rnd, x, y, w, h):
    d.rectangle([x, y, x + w - 1, y + h - 1], fill="#b3120f")
    # faint wear streaks, as the canvas version scatters them
    for _ in range(60):
        light = rnd.random() < 0.5
        a = int(rnd.random() * 0.05 * 255)
        c = (255, 255, 255, a) if light else (0, 0, 0, a)
        sx, sy = x + rnd.random() * w, y + rnd.random() * h
        d.rectangle([sx, sy, sx + 2 + rnd.random() * 30, sy + 1 + rnd.random() * 3], fill=c)


def main():
    rnd = random.Random(20261124)
    im = Image.new("RGB", (1024, 512), (0, 0, 0))
    d = ImageDraw.Draw(im, "RGBA")
    for i, t in enumerate(ORDERS_V):
        x = i * 128
        cloth(d, rnd, x, 0, 128, 512)
        size = min(84, 480 // len(t))
        f = font(size)
        for k, ch in enumerate(t):
            cy = 256 + (k - (len(t) - 1) / 2) * size * 1.02
            d.text((x + 64, cy), ch, font=f, fill="#ffe94a", anchor="mm")
    for i, t in enumerate(ORDERS_H):
        y = i * 128
        cloth(d, rnd, 512, y, 512, 128)
        size = 56
        f = font(size)
        while d.textlength(t, font=f) > 480 and size > 20:
            size -= 2
            f = font(size)
        d.text((768, y + 64), t, font=f, fill="#ffffff", anchor="mm")
    OUT.mkdir(parents=True, exist_ok=True)
    im.save(OUT / "orders.png", optimize=True)
    print("wrote", OUT / "orders.png")


if __name__ == "__main__":
    main()
