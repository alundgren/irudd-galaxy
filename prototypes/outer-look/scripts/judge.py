"""Score pictures against the concept: numbers per picture and a side-by-side sheet.

    python -I scripts/judge.py <out.jpg> <label>=<image>[@x0,y0,x1,y1] ...

Each picture (optionally cropped to a box) is measured the same way:
- chroma: mean CIE LCh chroma, how saturated the paint is;
- marks: share of painted (non-sky) pixels on a fine drawn edge (|Laplacian| of lightness above 18 at full
  size, on a picture scaled down to the smallest picture's height first), how much small drawn detail there is;
- warm light: share of pixels that are bright warm light (lamps, windows, glow);
- sky: median colour of the darkest quarter, and its CIE76 delta E to the first picture's
  (under ~10 reads as the same paint, over ~25 as a different colour; same scale as the
  style kit's palette.py --regions).
The first picture is the reference. The sheet puts them side by side with their numbers.
Numbers stop drift; they don't prove a look. The owner's eye decides.
"""
import sys

import cv2
import numpy as np


def lab_img(bgr):
    return cv2.cvtColor(bgr.astype(np.float32) / 255, cv2.COLOR_BGR2Lab)


def measure(bgr, height):
    s = height / bgr.shape[0]
    if abs(s - 1) > 0.01:
        bgr = cv2.resize(bgr, None, fx=s, fy=s, interpolation=cv2.INTER_AREA)
    lab = lab_img(bgr)
    L = lab[..., 0]
    chroma = float(np.hypot(lab[..., 1], lab[..., 2]).mean())
    lap = np.abs(cv2.Laplacian(cv2.GaussianBlur(L, (0, 0), 0.6), cv2.CV_32F))
    hsv = cv2.cvtColor(bgr, cv2.COLOR_BGR2HSV)
    warm = float(((hsv[..., 0] >= 10) & (hsv[..., 0] <= 32) & (hsv[..., 2] > 215) & (hsv[..., 1] > 90)).mean())
    dark = L <= np.percentile(L, 25)
    sky = np.median(bgr[dark], axis=0)
    # marks count only on painted things, not on empty sky, so crops with more sky compare fairly
    sky_lab = lab_img(sky.reshape(1, 1, 3).astype(np.uint8))[0, 0]
    thing = np.linalg.norm(lab - sky_lab, axis=2) > 14
    thing = cv2.erode(thing.astype(np.uint8), np.ones((3, 3), np.uint8)) > 0
    marks = float((lap[thing] > 18).mean()) if thing.any() else 0.0
    return {"chroma": chroma, "marks": marks, "warm": warm, "sky": sky}


def delta_e(a, b):
    la = lab_img(a.reshape(1, 1, 3))[0, 0]
    lb = lab_img(b.reshape(1, 1, 3))[0, 0]
    return float(np.linalg.norm(la - lb))


out = sys.argv[1]
items = []
for arg in sys.argv[2:]:
    label, rest = arg.split("=", 1)
    path, _, box = rest.partition("@")
    im = cv2.imread(path)
    if box:
        x0, y0, x1, y1 = map(int, box.split(","))
        im = im[y0:y1, x0:x1]
    items.append([label, im, None])
height = min(im.shape[0] for _, im, _ in items)
for it in items:
    it[2] = measure(it[1], height)

ref = items[0][2]
tiles = []
print("%-28s %7s %7s %7s %9s" % ("picture", "chroma", "marks", "warm", "sky dE"))
for label, im, m in items:
    de = delta_e(m["sky"], ref["sky"])
    line = "%-28s %7.1f %6.1f%% %6.2f%% %9.1f" % (label, m["chroma"], m["marks"] * 100, m["warm"] * 100, de)
    print(line)
    th = 360
    t = cv2.resize(im, (int(im.shape[1] * th / im.shape[0]), th), interpolation=cv2.INTER_AREA)
    bar = np.full((54, t.shape[1], 3), 30, np.uint8)
    cv2.putText(bar, label, (8, 22), cv2.FONT_HERSHEY_SIMPLEX, 0.6, (230, 240, 250), 1, cv2.LINE_AA)
    cv2.putText(bar, "chroma %.0f  marks %.1f%%  warm %.1f%%  sky dE %.0f" % (m["chroma"], m["marks"] * 100,
                m["warm"] * 100, de), (8, 44), cv2.FONT_HERSHEY_SIMPLEX, 0.45, (170, 210, 230), 1, cv2.LINE_AA)
    tiles.append(np.vstack([t, bar]))
gap = np.full((tiles[0].shape[0], 8, 3), 15, np.uint8)
row = tiles[0]
for t in tiles[1:]:
    row = np.hstack([row, gap, t])
cv2.imwrite(out, row, [cv2.IMWRITE_JPEG_QUALITY, 88])
print("sheet:", out)
