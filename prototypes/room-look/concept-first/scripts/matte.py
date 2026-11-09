"""Difference matte for foreground blockouts (plain Python).

A foreground blockout (Bram's head sphere, his torso blob, the mug's box) never matches the painted
silhouette. Inside it, near its edge, some concept pixels are Bram and some are the wall behind him.
Tell them apart by comparing the concept with what the background objects alone show there (their
bake, which the guard band filled from the wall's own paint): where the concept looks like the
background, the foreground is transparent.

usage: python -I scripts/matte.py            (after piece1 delight + bake)
writes pieces/01-lamp-corner/matte.png (concept-sized, white = foreground paint) and matte-check.png
"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from PIL import Image, ImageFilter
import tools
import piece1_targets as T

S = tools.STUDY
p0 = os.path.join(S, "pieces", "00-layout")
p1 = os.path.join(S, "pieces", "01-lamp-corner")
BAND = 14  # px each side of the blockout outline that the matte decides

bg_dir = os.path.join(p1, "bg-albedo")
background = [n for n in T.TARGETS if not n.startswith(T.FOREGROUND)]
tools.blender("relight.py", os.path.join(p1, "bake"), os.path.join(p1, "style.json"), bg_dir, "albedo",
              ",".join(background), blend=os.path.join(p1, "bake", "baked.blend"))
bg = np.asarray(Image.open(os.path.join(bg_dir, "albedo.png")).convert("RGB")).astype(float) / 255
delit = np.asarray(Image.open(os.path.join(p1, "concept-delit.png")).convert("RGB")).astype(float) / 255
ids = np.asarray(Image.open(os.path.join(p0, "id.png")).convert("RGB"))[:, :, 0].astype(int)
names = {n: int(k) for k, n in json.load(open(os.path.join(p0, "id.json"))).items()}
fg = np.isin(ids, [k for n, k in names.items() if n.startswith(T.FOREGROUND)])


def morph(mask, r, grow):
    f = ImageFilter.MaxFilter(2 * r + 1) if grow else ImageFilter.MinFilter(2 * r + 1)
    return np.asarray(Image.fromarray((mask * 255).astype(np.uint8)).filter(f)) > 127


inner, outer = morph(fg, BAND, False), morph(fg, BAND, True)
# colour distance, with brightness weighted down (the bake is delit, both sides are delit)
# (tried 10:40: blurring both first did not break the self-confirming loop; see LOG piece 5)
d = np.sqrt(((delit - bg) ** 2).sum(-1))
a = np.clip((d - 0.06) / (0.20 - 0.06), 0, 1)
a = np.asarray(Image.fromarray((a * 255).astype(np.uint8)).filter(ImageFilter.MedianFilter(5))).astype(float) / 255
# decide everywhere inside the stand-ins, not only near their outline: the board seen between Bram's
# raised arm and his head lies deep inside the union of blockouts. Close small holes so chance
# matches inside the jacket (dark cloth vs dark frame) don't punch through.
a8 = Image.fromarray((a * 255).astype(np.uint8))
a = np.asarray(a8.filter(ImageFilter.MaxFilter(9)).filter(ImageFilter.MinFilter(9))).astype(float) / 255
matte = np.where(outer, a, 0.0)
# hand-traced overrides (concept px), for regions the difference test cannot decide because its own
# guard hid them from the bake: the board between Bram's face, fist and raised sleeve is background
from PIL import ImageDraw
EXCLUDE = [[(548, 280), (720, 280), (720, 640), (655, 640), (632, 545), (617, 500), (585, 493),
            (583, 440), (560, 425), (537, 420), (544, 350)]]
ex = Image.new("L", (matte.shape[1], matte.shape[0]), 0)
for poly in EXCLUDE:
    ImageDraw.Draw(ex).polygon(poly, fill=255)
matte = np.where(np.asarray(ex) > 127, 0.0, matte)
matte = np.asarray(Image.fromarray((matte * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(1.0)))
Image.fromarray(matte).save(os.path.join(p1, "matte.png"))
# painted ID: pixels inside a stand-in that the matte says are background take the label of the
# background next to them, so the second delight evens the wall out as wall (the blockout ellipse
# otherwise left a seam in the wall's delit paint)
lab = ids.copy()
todo = fg & (matte < 128)
known = ~fg
for _ in range(80):
    if not todo.any():
        break
    for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
        sl = np.roll(lab, (dy, dx), (0, 1))
        sk = np.roll(known, (dy, dx), (0, 1))
        take = todo & sk
        lab[take] = sl[take]
        known = known | take
        todo = todo & ~take
idrgb = np.asarray(Image.open(os.path.join(p0, "id.png")).convert("RGB")).copy()
idrgb[..., 0] = lab
Image.fromarray(idrgb).save(os.path.join(p1, "id-painted.png"))
# with a painted silhouette in hand, the background only needs a thin guard around the paint itself
gm = morph(matte > 127, 4, True)
Image.fromarray(((~gm) * 255).astype(np.uint8)).save(os.path.join(p1, "guard-matte.png"))
con = np.asarray(Image.open(os.path.join(S, "concept", "room.png")).convert("RGB")).astype(float)
chk = con * (0.25 + 0.75 * matte[..., None] / 255) + np.array([0, 60, 0]) * (1 - matte[..., None] / 255) * outer[..., None]
Image.fromarray(np.clip(chk, 0, 255).astype(np.uint8)).save(os.path.join(p1, "matte-check.png"))
print("ok matte; band px", int((outer & ~inner).sum()), "kept", round(float((matte[outer & ~inner] > 127).mean()), 3))
