# Step 7. Comparison sheets from renders/work/ (numpy only; Blender's Python works):
#   <python> -I scripts/make_sheets.py <repo root> [prefix]
# Writes renders/<prefix>_*.png. Labels are drawn into the images.
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import draw  # noqa: E402
import landmarks as L  # noqa: E402
import pngio  # noqa: E402

LAB = os.path.dirname(HERE)
W = os.path.join(LAB, "renders", "work")
root = sys.argv[1]
PRE = sys.argv[2] if len(sys.argv) > 2 else "cp1"
y0, y1, x0, x1 = L.CROP
concept = pngio.read(os.path.join(root, L.REFERENCE))[y0:y1, x0:x1].astype(np.float32)


def r(name):
    p = os.path.join(W, name + ".png")
    return pngio.read(p).astype(np.float32) if os.path.exists(p) else None


def save(name, img):
    path = os.path.join(LAB, "renders", f"{PRE}_{name}.png")
    pngio.write(path, img)
    print(path)


def lab(img, t, s=2):
    return draw.labelled(img, t, s)


def size(img, w, h):
    return pngio.resize(img, w, h)


# 1. A / B / C beside the concept at the concept camera: 568x900 and 240x380.
cols = [("CONCEPT", concept), ("A CLAY", r("A_clay_concept")), ("B PAINT+INK", r("B_concept_concept")),
        ("C B+GEOMETRY FIT", r("C_concept_concept"))]
cols = [(t, i) for t, i in cols if i is not None]
save("abc_concept_view_568x900", draw.hstack([lab(i, t) for t, i in cols]))
save("abc_talk_240x380", draw.hstack([lab(size(i, 240, 380), t, 1) for t, i in cols]))
blend = [("CONCEPT", concept)]
for t, n in (("50% B", "B_concept_concept"), ("50% C", "C_concept_concept")):
    i = r(n)
    if i is not None:
        blend.append((t, concept * 0.5 + i * 0.5))
save("blend_concept_vs_BC", draw.hstack([lab(i, t) for t, i in blend]))

# 2. Clay views, A on top, C below.
views = ("front", "profile", "yaw-20", "concept", "yaw20")
rows = []
for a in ("A_clay", "C_clay"):
    ims = []
    for v in views:
        i = r(f"{a}_clay_{v}")
        if i is None:
            continue
        if v == "concept":
            i = i[60:760]          # same height as the 600x700 views
        ims.append(lab(i, f"{a.split('_')[0]} {v.upper().replace('YAW', 'YAW ')}"))
    if ims:
        rows.append(draw.hstack(ims))
if rows:
    save("clay_A_vs_C_front_profile_yaw", draw.vstack(rows))

# 3. Style under held-out views and a changed key light.
rows = []
for a in ("B", "C"):
    ims = []
    for rig, v, t in (("concept", "yaw-20", "YAW -20"), ("concept", "concept", "FIT VIEW"),
                      ("concept", "yaw20", "YAW +20"), ("swapped", "concept", "KEY SWAPPED")):
        i = r(f"{a}_{rig}_{v}")
        if i is not None:
            ims.append(lab(size(i, 379, 600), f"{a} {t}"))
    if ims:
        rows.append(draw.hstack(ims))
if rows:
    save("style_heldout_yaw_and_light", draw.vstack(rows))

# 4. Raw projection seen from the fitted view and +-20 degrees (green = never seen).
ims = [lab(size(i, 379, 600), t) for t, i in (("RAW YAW -20", r("raw_clay_yaw-20")), ("RAW FIT VIEW", r("raw_clay_concept")),
                                             ("RAW YAW +20", r("raw_clay_yaw20"))) if i is not None]
if ims:
    save("raw_projection_yaw", draw.hstack(ims))

# 5. Where the 3D landmark points sit (the inferred depth): A and C, front, profile, below.
rows = []
for a in ("A", "C"):
    ims = [lab(i, f"{a} {v.upper().replace('_', ' ')}") for v in ("front", "profile_right", "below")
           for i in [r(f"{a}_marks_{v}")] if i is not None]
    if ims:
        rows.append(draw.hstack(ims))
if rows:
    save("landmarks_3d_A_vs_C", draw.vstack(rows))

# 6. Paint layers in UV space and the de-lit concept they were taken from.
ims = [lab(size(i, 512, 512), t) for t, i in (("RAW PROJECTION", r("layer_raw")), ("SEEN WEIGHT", r("layer_weight")),
                                              ("PAINT LAYER B", r("layer_paint")), ("PAINT LAYER C", r("layer_paint_C")))
       if i is not None]
ims += [lab(size(i, 324, 512), t) for t, i in (("CONCEPT DE-LIT", r("concept_delit")),) if i is not None]
if ims:
    save("paint_layers_uv", draw.hstack(ims))
