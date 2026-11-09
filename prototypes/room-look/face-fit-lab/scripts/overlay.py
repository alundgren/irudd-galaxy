# Landmark overlays (2D only; run with any Python that has numpy, e.g. Blender's own):
#   <python> -I scripts/overlay.py <repo root> concept            -> renders/landmarks_concept.png
#   <python> -I scripts/overlay.py <repo root> fit <json> <tag> [<render.png>]
#        -> renders/<tag>_landmark_overlay.png: concept marks (cyan crosses), model points
#           through the fitted camera (magenta rings) and the error line between them,
#           drawn over the concept and, if given, over the render.
import json
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import draw  # noqa: E402
import landmarks as L  # noqa: E402
import pngio  # noqa: E402

LAB = os.path.dirname(HERE)
ROLE_COL = {"anchor": (0, 255, 255), "form": (255, 140, 40), "expr": (255, 220, 0), "paint": (140, 255, 120)}
FACE = (80, 190, 480, 600)   # x0, y0, x1, y1 of the face region in crop px
SC = 2


def crop(root):
    y0, y1, x0, x1 = L.CROP
    return pngio.read(os.path.join(root, L.REFERENCE))[y0:y1, x0:x1].astype(np.float32)


def zoom(img):
    x0, y0, x1, y1 = FACE
    return pngio.resize(img[y0:y1, x0:x1], (x1 - x0) * SC, (y1 - y0) * SC)


def to_z(p):
    return ((p[0] - FACE[0]) * SC, (p[1] - FACE[1]) * SC)


def concept_overlay(root):
    img = zoom(crop(root)) * 0.75
    names = list(L.CONCEPT)
    for i, k in enumerate(names):
        p, conf, role = L.CONCEPT[k]
        q = to_z(p)
        draw.ring(img, q, ROLE_COL[role], r=int(conf * SC), w=1)
        draw.cross(img, q, ROLE_COL[role], r=4, w=2)
        draw.text(img, str(i + 1), int(q[0]) + 5, int(q[1]) - 16, 1, ROLE_COL[role], (0, 0, 0))
    leg = np.full((img.shape[0], 300, 3), (30, 24, 36), np.float32)
    for i, k in enumerate(names):
        p, conf, role = L.CONCEPT[k]
        draw.text(leg, f"{i + 1:2d} {k} {conf}PX", 6, 8 + i * 18, 2 if len(k) < 13 else 1, ROLE_COL[role], None)
    draw.text(leg, "CYAN ANCHOR  ORANGE FORM", 6, img.shape[0] - 56, 1, (240, 230, 210), None)
    draw.text(leg, "YELLOW EXPR", 6, img.shape[0] - 40, 1, (240, 230, 210), None)
    draw.text(leg, "GREEN PAINT  RING = CONF", 6, img.shape[0] - 24, 1, (240, 230, 210), None)
    out = draw.hstack([img, leg])
    path = os.path.join(LAB, "renders", "landmarks_concept.png")
    pngio.write(path, out)
    return path


def fit_overlay(root, fit_json, tag, render=None):
    d = json.load(open(fit_json))
    panels = []
    bases = [("CONCEPT", crop(root))]
    if render:
        bases.append(("RENDER", pngio.read(render).astype(np.float32)))
    for title, base in bases:
        img = zoom(base) * 0.8
        for i, (k, v) in enumerate(d["landmarks"].items()):
            c, m = to_z(v["concept"]), to_z(v["model"])
            col = ROLE_COL[v["role"]]
            draw.line(img, c, m, (255, 80, 200), 1)
            draw.cross(img, c, col, r=4, w=2)
            draw.ring(img, m, (255, 80, 200), r=4, w=1)
            draw.text(img, str(i + 1), int(c[0]) + 5, int(c[1]) - 14, 1, col, (0, 0, 0))
        panels.append(draw.labelled(img, title))
    leg = np.full((panels[0].shape[0], 330, 3), (30, 24, 36), np.float32)
    draw.text(leg, f"ANCHOR RMS {d['anchor_rms_px']:.1f}PX = {d['anchor_rms_pct_eye_sep']:.1f}%", 6, 8, 1, (240, 230, 210), None)
    draw.text(leg, f"EYE SEP {d['eye_separation_px']:.0f}PX  LENS {d['camera']['lens_mm']:.0f}MM", 6, 22, 1, (240, 230, 210), None)
    for i, (k, v) in enumerate(d["landmarks"].items()):
        col = ROLE_COL[v["role"]]
        draw.text(leg, f"{i + 1:2d} {k:13s}{v['err_px']:5.1f} {v['err_pct_eye_sep']:5.1f}%", 6, 44 + i * 16, 1, col, None)
    out = draw.hstack(panels + [leg])
    path = os.path.join(LAB, "renders", f"{tag}_landmark_overlay.png")
    pngio.write(path, out)
    return path


if __name__ == "__main__":
    root, mode = sys.argv[1], sys.argv[2]
    if mode == "concept":
        print(concept_overlay(root))
    else:
        print(fit_overlay(root, sys.argv[3], sys.argv[4], sys.argv[5] if len(sys.argv) > 5 else None))
