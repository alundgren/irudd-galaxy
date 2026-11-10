"""Cut the concept paintings into layers: sprites with alpha, and a clean sky plate.

    python -I scripts/cut.py <concept folder> <out folder>

The concept folder holds flight-preferred.png and station-exterior.png (kept outside the
repo). Each sprite is cut by GrabCut seeded with a hand-picked box, then its alpha edge is
softened by one pixel. The sky plate is the flight concept with every cut object inpainted.
The cut layers stand in for the paintings an image tool or a painter would deliver per
asset; the pipeline after this point is the same either way.
"""
import os
import sys

import cv2
import numpy as np

SRC, OUT = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)

# name: (x0, y0, x1, y1) in flight-preferred.png
FLIGHT = {
    "rock_iron_big": (1268, 38, 1590, 330),
    "rock_iron_small": (1062, 160, 1208, 285),
    "rock_iron_tiny": (1210, 350, 1288, 418),
    "rock_ice": (1352, 368, 1612, 538),
    "rock_iron_mid": (1132, 552, 1275, 675),
    "rock_glassite": (1318, 632, 1558, 820),
    "rock_iron_low": (98, 638, 385, 880),
    "rock_iron_flat": (522, 732, 695, 860),
    "rock_iron_long": (885, 715, 1152, 888),
    "station_far": (28, 28, 835, 552),
    "ship_side": (818, 505, 950, 600),
}
# station-exterior.png: the whole station, minus the caption at the bottom left
STATION = {"station": (12, 20, 1665, 925)}


def grabcut(img, box, iters=6):
    x0, y0, x1, y1 = box
    pad = 6
    sub = img[max(0, y0 - pad):y1 + pad, max(0, x0 - pad):x1 + pad].copy()
    mask = np.zeros(sub.shape[:2], np.uint8)
    rect = (pad, pad, x1 - x0, y1 - y0)
    bgd, fgd = np.zeros((1, 65), np.float64), np.zeros((1, 65), np.float64)
    cv2.grabCut(sub, mask, rect, bgd, fgd, iters, cv2.GC_INIT_WITH_RECT)
    fg = np.where((mask == cv2.GC_FGD) | (mask == cv2.GC_PR_FGD), 255, 0).astype(np.uint8)
    # keep the components that matter, drop specks of starfield
    n, lab, stats, _ = cv2.connectedComponentsWithStats(fg)
    keep = np.zeros_like(fg)
    big = stats[1:, cv2.CC_STAT_AREA].max() if n > 1 else 0
    for i in range(1, n):
        if stats[i, cv2.CC_STAT_AREA] >= max(30, big * 0.004):
            keep[lab == i] = 255
    keep = cv2.morphologyEx(keep, cv2.MORPH_CLOSE, np.ones((3, 3), np.uint8))
    alpha = cv2.GaussianBlur(keep, (3, 3), 0.8)
    rgba = np.dstack([sub, alpha])
    return rgba, keep, (max(0, x0 - pad), max(0, y0 - pad))


def save_rgba(path, rgba_bgr):
    cv2.imwrite(path, rgba_bgr)


flight = cv2.imread(os.path.join(SRC, "flight-preferred.png"))
holes = np.zeros(flight.shape[:2], np.uint8)
for name, box in FLIGHT.items():
    rgba, keep, (ox, oy) = grabcut(flight, box)
    save_rgba(os.path.join(OUT, name + ".png"), rgba)
    h, w = keep.shape
    holes[oy:oy + h, ox:ox + w] |= keep
    print(name, "cut", w, "x", h, "coverage %.2f" % (keep.mean() / 255))

# the ship's exhaust flame is painted, not part of the hull: inpaint it too
holes[540:575, 770:835] = 255
holes = cv2.dilate(holes, np.ones((9, 9), np.uint8))
sky = cv2.inpaint(flight, holes, 9, cv2.INPAINT_TELEA)
# inpainting leaves smears; a heavy blur of the filled area only keeps the nebula soft
blur = cv2.GaussianBlur(sky, (0, 0), 14)
m = cv2.GaussianBlur(holes, (0, 0), 6)[..., None] / 255.0
sky = (sky * (1 - m) + blur * m).astype(np.uint8)
cv2.imwrite(os.path.join(OUT, "sky_plate.png"), sky)

station = cv2.imread(os.path.join(SRC, "station-exterior.png"))
for name, box in STATION.items():
    rgba, keep, _ = grabcut(station, box, iters=8)
    # the caption and its rule line are text, not station
    rgba[880 - 14:, :430 - 6, 3] = 0
    save_rgba(os.path.join(OUT, name + ".png"), rgba)
    print(name, "cut coverage %.2f" % (keep.mean() / 255))
