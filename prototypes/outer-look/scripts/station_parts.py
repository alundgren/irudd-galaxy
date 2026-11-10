"""Can a station be a recipe of painted part plates? Cut the plate into parts, rebuild another layout.

    python -I scripts/station_parts.py <layers folder> <out.jpg>

Parts are cut from the station plate (cut/station.png) by hand-drawn outlines in plate
pixels. A recipe places parts on the play plane; they are drawn far to near (by their base
line), as the engine would sort sprites. The sheet shows the painted plate, the parts, and
a new layout built from them, so the joins can be judged.
"""
import os
import sys

import cv2
import numpy as np

LAYERS, OUT = sys.argv[1], sys.argv[2]
plate = cv2.imread(os.path.join(LAYERS, "cut", "station.png"), cv2.IMREAD_UNCHANGED)

# outlines in plate pixels, each a building on its own patch of deck
PARTS = {
    "hall": [(720, 170), (1180, 170), (1225, 560), (1110, 690), (800, 700), (700, 560)],
    "greenhouse": [(1190, 380), (1540, 380), (1560, 640), (1420, 760), (1210, 720), (1170, 560)],
    "galley": [(1250, 60), (1580, 60), (1600, 330), (1260, 340), (1230, 230)],
    "hangar": [(240, 20), (790, 20), (800, 330), (620, 360), (330, 330), (210, 260)],
    "dock": [(10, 360), (690, 360), (760, 600), (620, 790), (10, 790)],
}


def part(name):
    m = np.zeros(plate.shape[:2], np.uint8)
    cv2.fillPoly(m, [np.array(PARTS[name], np.int32)], 255)
    m = cv2.GaussianBlur(m, (0, 0), 2)
    p = plate.copy()
    p[..., 3] = (p[..., 3].astype(np.float32) * m / 255).astype(np.uint8)
    x, y, w, h = cv2.boundingRect(np.array(PARTS[name], np.int32))
    return p[y:y + h, x:x + w]


def place(canvas, rgba, cx, cy):
    h, w = rgba.shape[:2]
    x0, y0 = int(cx - w / 2), int(cy - h / 2)
    xs, ys = max(0, x0), max(0, y0)
    xe, ye = min(canvas.shape[1], x0 + w), min(canvas.shape[0], y0 + h)
    s = rgba[ys - y0:ye - y0, xs - x0:xe - x0].astype(np.float32) / 255
    a = s[..., 3:]
    canvas[ys:ye, xs:xe] = canvas[ys:ye, xs:xe] * (1 - a) + s[..., :3] * a


sky = np.zeros((900, 1600, 3), np.float32)
sky[:] = np.array([76, 31, 59], np.float32) / 255  # BGR of #3b1f4c

# recipe B: a different station from the same parts, laid out along a straight quay
RECIPE = [("hangar", 360, 230), ("galley", 1270, 200), ("hall", 800, 420), ("greenhouse", 360, 560),
          ("dock", 1150, 640)]
canvas = sky.copy()
for name, x, y in sorted(RECIPE, key=lambda r: r[2]):
    place(canvas, part(name), x, y)

# sheet: the plate, the parts, the new layout
def fit(img, h):
    if img.shape[2] == 4:
        a = img[..., 3:].astype(np.float32) / 255
        img = (img[..., :3].astype(np.float32) / 255 * a + sky[0, 0] * (1 - a))
    img = (np.clip(img, 0, 1) * 255).astype(np.uint8)
    return cv2.resize(img, (int(img.shape[1] * h / img.shape[0]), h), interpolation=cv2.INTER_AREA)


left = fit(plate, 450)
parts = np.hstack([fit(part(n), 180) for n in PARTS])
right = fit((canvas * 255).astype(np.uint8)[..., :3].astype(np.float32) / 255, 450)
top = np.hstack([left, np.full((450, 12, 3), 20, np.uint8), right])
parts = cv2.resize(parts, (top.shape[1], int(parts.shape[0] * top.shape[1] / parts.shape[1])))
cv2.imwrite(OUT, np.vstack([top, parts]), [cv2.IMWRITE_JPEG_QUALITY, 86])
print("sheet:", OUT)
