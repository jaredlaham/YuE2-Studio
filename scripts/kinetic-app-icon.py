"""Kinetic Studio app icon on Apple's macOS icon grid (1024 canvas, 824 body at 100).

Draws desktop/src-tauri/icons/kinetic/icon.png from Jared's artwork (four
overlapping gradient circles on black). Usage: python3 scripts/kinetic-app-icon.py out.png
The smaller PNGs and icon.icns are resized from that 1024 image with Pillow.
"""
import sys
import numpy as np
from PIL import Image, ImageFilter

S = 4                      # supersampling
N = 1024 * S
out = sys.argv[1]

yy, xx = np.mgrid[0:N, 0:N].astype(np.float32)
x = (xx + 0.5) / S
y = (yy + 0.5) / S

# Body: Apple's continuous-corner rounded square, approximated by a superellipse.
BODY, C = 824.0, 512.0
half = BODY / 2
n = 5.0
body = (np.abs((x - C) / half) ** n + np.abs((y - C) / half) ** n) <= 1.0

# Body fill: near-black with a soft lift at the top, like the artwork's black.
t = (y - (C - half)) / BODY
bg = np.stack([np.interp(t, [0, 1], [0x24, 0x06]),
               np.interp(t, [0, 1], [0x24, 0x06]),
               np.interp(t, [0, 1], [0x28, 0x08])], -1)

# Mark: four overlapping circles, proportions measured from the artwork
# (radius 54, centres 49 off the middle, 206 across) scaled to 450 across,
# inside the grid's central keyline area.
scale = 450.0 / 206.0
r, s = 54.0 * scale, 49.0 * scale
mark = np.zeros((N, N), bool)
for cx, cy in [(C - s, C - s), (C + s, C - s), (C - s, C + s), (C + s, C + s)]:
    mark |= (x - cx) ** 2 + (y - cy) ** 2 <= r ** 2

# Four-corner gradient sampled from the artwork: pink, blue, yellow, green.
TL, TR = np.array([250, 186, 250.]), np.array([34, 150, 255.])
BL, BR = np.array([252, 252, 14.]), np.array([112, 246, 72.])
ext = s + r
u = np.clip((x - (C - ext)) / (2 * ext), 0, 1)[..., None]
v = np.clip((y - (C - ext)) / (2 * ext), 0, 1)[..., None]
sm = lambda a: a * a * (3 - 2 * a)
u, v = sm(sm(u)), sm(sm(v))
grad = (TL * (1 - u) + TR * u) * (1 - v) + (BL * (1 - u) + BR * u) * v

rgb = np.where(mark[..., None], grad, bg)
alpha = body.astype(np.float32)
img = np.dstack([rgb * alpha[..., None], alpha * 255]).astype(np.float32)

big = Image.fromarray(img.clip(0, 255).astype(np.uint8), 'RGBA')
icon = big.resize((1024, 1024), Image.LANCZOS)

# Drop shadow under the body, per the grid template.
shadow_alpha = Image.fromarray((body * 255).astype(np.uint8)).resize((1024, 1024), Image.LANCZOS)
shadow = Image.new('RGBA', (1024, 1024), (0, 0, 0, 0))
shadow.putalpha(shadow_alpha.point(lambda a: int(a * 0.35)))
shadow = shadow.transform((1024, 1024), Image.AFFINE, (1, 0, 0, 0, 1, -12)).filter(ImageFilter.GaussianBlur(14))

canvas = Image.alpha_composite(Image.new('RGBA', (1024, 1024), (0, 0, 0, 0)), shadow)
canvas = Image.alpha_composite(canvas, icon)
canvas.save(out)
