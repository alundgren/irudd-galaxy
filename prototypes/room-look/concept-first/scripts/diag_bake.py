"""Diagnostic: compare wall_back bake preview texels with the delit concept at the same wall points."""
import sys, os
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
S = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
F = 35 / 36 * 1083
CAM = (0.0, -2.29, 1.58)
cy = 893 / 2 - 0.077 * 1083
delit = np.asarray(Image.open(os.path.join(S, "pieces/01-lamp-corner/concept-delit.png")).convert("RGB")).astype(int)
tex = np.asarray(Image.open(os.path.join(S, "pieces/01-lamp-corner/bake/wall_back.png")).convert("RGB")).astype(int)
ren = np.asarray(Image.open(os.path.join(S, "pieces/01-lamp-corner/albedo.png")).convert("RGB")).astype(int)
x0, x1, z0, z1 = -0.42, 2.0, 0.0, 3.2
H, W = tex.shape[:2]
for px, py in [(1040, 300), (1000, 120), (960, 600), (700, 60), (1060, 450)]:
    depth = 0 - CAM[1]
    X = (px - 1083 / 2) * depth / F
    Z = CAM[2] - (py - cy) * depth / F
    u, v = (X - x0) / (x1 - x0), (Z - z0) / (z1 - z0)
    tx, ty = int(u * (W - 1)), int((1 - v) * (H - 1))
    print((px, py), "delit", delit[py, px], "tex", tex[ty, tx], "render", ren[py, px])
