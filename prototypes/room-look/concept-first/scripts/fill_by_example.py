"""Fill texels the concept never showed with patches of the same object's seen paint.

usage: python -I fill_by_example.py <bake_dir> name[,name...] [tile_px=64] [seed=1]
Reads  <bake_dir>/<name>.png (albedo with smooth push-pull fill) and <name>-seen.png (0..1 seen weight).
Writes <bake_dir>/<name>.png (filled), keeps the smooth version as <name>-smooth.png and writes
<name>-fillmap.png (which source patch each filled tile came from, for inspection).

Method (greedy image quilting, scanline order):
  - candidate sources: tile-sized windows (with overlap border) that are >= 97% seen
  - for each tile that is >= 25% unseen: pick, among a random sample of candidates, the one whose
    top and left overlap best matches the pixels already there (seen paint or earlier fills), plus a
    colour term that keeps the smooth fill's local mean (so gradients continue)
  - shift the patch's mean colour to the smooth fill's local mean, feather it into the overlap,
    and never overwrite seen texels (blend by the seen weight)
Search runs at half resolution; pasting at full resolution.
"""
import os, sys, shutil
import numpy as np
from PIL import Image

bake_dir, names = sys.argv[1], sys.argv[2].split(",")
T = int(sys.argv[3]) if len(sys.argv) > 3 else 64
rng = np.random.default_rng(int(sys.argv[4]) if len(sys.argv) > 4 else 1)
O = max(4, T // 4)
N_CAND = 1500
MODE = sys.argv[5] if len(sys.argv) > 5 else "detail"   # "detail" (default) or "patch"
DETAIL_ONLY = MODE == "detail"
DETAIL_SIGMA = 5.0      # px: detail finer than this is copied
DETAIL_GAIN = 0.8


def load(p):
    return np.asarray(Image.open(p).convert("RGB")).astype(np.float32) / 255


def fill(name):
    src = os.path.join(bake_dir, name + ".png")
    smooth_p = os.path.join(bake_dir, name + "-smooth.png")
    if not os.path.exists(smooth_p):
        shutil.copy(src, smooth_p)
    A = load(smooth_p)
    w = load(os.path.join(bake_dir, name + "-seen.png"))[..., 0]
    H, W = w.shape
    if DETAIL_ONLY:
        # copy only fine detail (grain, scratches, wear) onto the smooth colour; features such as
        # pipes and clamps should be modelled on purpose, not hallucinated by copying patches
        from PIL import ImageFilter
        img = Image.fromarray((A * 255).astype(np.uint8))
        low = np.asarray(img.filter(ImageFilter.GaussianBlur(DETAIL_SIGMA))).astype(np.float32) / 255
        HP = (A - low) * DETAIL_GAIN
    out = A.copy()
    known = w.copy()                       # 1 = trustworthy pixel (seen, or already filled)
    # candidates: top-left corners of (T+O)x(T+O) windows that the concept saw
    step = max(4, T // 8)
    ys, xs = np.mgrid[0:H - T - O:step, 0:W - T - O:step]
    cands = []
    for y, x in zip(ys.ravel(), xs.ravel()):
        if w[y:y + T + O, x:x + T + O].mean() >= 0.97:
            cands.append((y, x))
    if len(cands) < 10:
        print(name, "too few seen candidates:", len(cands))
        return {"candidates": len(cands), "tiles": 0}
    cands = np.array(cands)
    # half-resolution copies for the search
    def half(a):
        return a[: a.shape[0] // 2 * 2, : a.shape[1] // 2 * 2].reshape(a.shape[0] // 2, 2, a.shape[1] // 2, 2, -1).mean((1, 3))
    Ah = half(A)
    th, oh = T // 2, O // 2
    fillmap = np.zeros((H, W, 3), np.float32)
    tiles = 0
    for ty in range(0, H - O, T):
        for tx in range(0, W - O, T):
            y0, x0 = max(0, ty - O), max(0, tx - O)
            y1, x1 = min(H, ty + T), min(W, tx + T)
            if (1 - w[ty:y1, tx:x1]).mean() < 0.25:
                continue
            tiles += 1
            pick = cands[rng.choice(len(cands), min(N_CAND, len(cands)), replace=False)]
            hh, ww_ = (y1 - y0), (x1 - x0)
            # target overlap (top band + left band), current output at half res
            outh = half(out)
            kh = half(known[..., None])[..., 0]
            ty0, tx0 = y0 // 2, x0 // 2
            th_, tw_ = hh // 2, ww_ // 2
            tgt = outh[ty0:ty0 + th_, tx0:tx0 + tw_]
            kmask = kh[ty0:ty0 + th_, tx0:tx0 + tw_]
            band = np.zeros_like(kmask)
            band[: (ty - y0) // 2 + 1, :] = 1
            band[:, : (tx - x0) // 2 + 1] = 1
            m = (band * kmask)[..., None]
            smooth_mean = A[ty:y1, tx:x1].reshape(-1, 3).mean(0)
            best, best_cost = None, 1e9
            for cy, cx in pick:
                patch = Ah[cy // 2: cy // 2 + th_, cx // 2: cx // 2 + tw_]
                if patch.shape[:2] != (th_, tw_):
                    continue
                # compare after shifting the patch's mean to the target's smooth mean
                shift = smooth_mean - patch.reshape(-1, 3).mean(0)
                d = ((patch + shift - tgt) ** 2 * m).sum() / max(m.sum(), 1)
                c = d + 0.5 * float((shift ** 2).sum())   # prefer patches that need little recolouring
                if c < best_cost:
                    best, best_cost = (cy, cx), c
            cy, cx = best
            if DETAIL_ONLY:
                patch = A[y0:y1, x0:x1] + HP[cy: cy + hh, cx: cx + ww_]
            else:
                patch = A[cy: cy + hh, cx: cx + ww_].copy()
                patch += smooth_mean - patch.reshape(-1, 3).mean(0)
            # feather into the overlap bands, never over seen texels
            ramp_y = np.clip(np.arange(hh) / max(1, ty - y0), 0, 1) if ty > y0 else np.ones(hh)
            ramp_x = np.clip(np.arange(ww_) / max(1, tx - x0), 0, 1) if tx > x0 else np.ones(ww_)
            feather = np.minimum(ramp_y[:, None], ramp_x[None, :])
            region_known = known[y0:y1, x0:x1]
            a = feather * (1 - w[y0:y1, x0:x1])                  # seen texels keep their paint
            a = np.where(region_known > 0.5, a * feather, a)
            out[y0:y1, x0:x1] = out[y0:y1, x0:x1] * (1 - a[..., None]) + patch * a[..., None]
            known[y0:y1, x0:x1] = np.maximum(known[y0:y1, x0:x1], a)
            fillmap[ty:y1, tx:x1] = (cy / H, cx / W, 0.5)
    Image.fromarray((np.clip(out, 0, 1) * 255 + 0.5).astype(np.uint8)).save(src)
    Image.fromarray((fillmap * 255).astype(np.uint8)).save(os.path.join(bake_dir, name + "-fillmap.png"))
    return {"candidates": int(len(cands)), "tiles": tiles}


for n in names:
    print(n, fill(n))
