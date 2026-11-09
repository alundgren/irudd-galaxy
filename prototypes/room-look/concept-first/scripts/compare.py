"""Per-object colour match between a render from the concept camera and the concept.
usage: python -I compare.py render.png id.png id.json concept.png obj,obj,...
Prints, per object: concept median sRGB, render median sRGB, ratio per channel, and overall
luminance ratio. Same camera, so the same pixels belong to the same object (render may be scaled)."""
import json, sys
import numpy as np
from PIL import Image

r_png, id_png, id_json, c_png, objs = sys.argv[1:6]
con = Image.open(c_png).convert("RGB")
ren = Image.open(r_png).convert("RGB").resize(con.size, Image.BILINEAR)
ids = np.asarray(Image.open(id_png).convert("RGB"))[:, :, 0].astype(int)
names = {n: int(k) for k, n in json.load(open(id_json)).items()}
C = np.asarray(con).astype(float) / 255
R = np.asarray(ren).astype(float) / 255
rows = []
allc, allr = [], []
for n in objs.split(","):
    m = ids == names[n]
    if m.sum() < 200:
        continue
    c, r = np.median(C[m], 0), np.median(R[m], 0)
    allc.append(C[m]); allr.append(R[m])
    print(f"{n:16s} concept {np.round(c, 2)} render {np.round(r, 2)} ratio {np.round(r / np.maximum(c, 1e-3), 2)}")
c = np.concatenate(allc); r = np.concatenate(allr)
w = np.array([0.2126, 0.7152, 0.0722])
print("ALL mean ratio rgb", np.round(r.mean(0) / c.mean(0), 3), "lum", round(float((r @ w).mean() / (c @ w).mean()), 3),
      "saturation concept/render", round(float((c.max(1) - c.min(1)).mean()), 3), round(float((r.max(1) - r.min(1)).mean()), 3))
