"""Remove the painter's broad lighting from the concept, per object, and measure the painter's
shadow tint and ink colour.

Idea: inside one object's mask, the painted image = albedo x illumination. Painted illumination
changes slowly across a surface (hotspots, falloff, soft shadows); albedo detail (ink lines, rivets,
scratches, printed labels) changes quickly. A blur that stays inside the mask estimates the slow part;
dividing it out leaves the fast part at the object's median brightness.

usage (library): delight(concept_png, id_png, id_json, out_png, sigma=28) -> style dict
"""
import json
import numpy as np
from PIL import Image


def gauss_kernel(sigma):
    r = int(3 * sigma)
    x = np.arange(-r, r + 1)
    k = np.exp(-(x * x) / (2 * sigma * sigma))
    return k / k.sum()


def blur(a, sigma):
    k = gauss_kernel(sigma)
    a = np.apply_along_axis(lambda r: np.convolve(r, k, mode="same"), 1, a)
    return np.apply_along_axis(lambda c: np.convolve(c, k, mode="same"), 0, a)


def masked_blur(a, m, sigma):
    """Normalised convolution: average of `a` over nearby pixels that are inside mask `m`."""
    num = blur(a * m, sigma)
    den = blur(m.astype(float), sigma)
    return num / np.maximum(den, 1e-6)


def lum(rgb):
    return rgb[..., 0] * 0.2126 + rgb[..., 1] * 0.7152 + rgb[..., 2] * 0.0722


def srgb_to_lin(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def lin_to_srgb(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def delight(concept_png, id_png, id_json, out_png, sigma=28, objects=None, clamp=(0.45, 2.2), annotations=()):
    """annotations: pixel rectangles (x0, y0, x1, y1) that are not part of the world (captions,
    labels drawn on the concept). They are written black, which the bake treats as unseen."""
    con = srgb_to_lin(np.asarray(Image.open(concept_png).convert("RGB")).astype(float) / 255)
    ids = np.asarray(Image.open(id_png).convert("RGB"))[:, :, 0].astype(int)
    names = {n: int(k) for k, n in json.load(open(id_json)).items()}
    out = con.copy()
    report = {}
    for n, k in names.items():
        if objects and n not in objects:
            continue
        m = ids == k
        if m.sum() < 50:
            continue
        L = lum(con)
        I = masked_blur(L, m, sigma)          # painted illumination estimate (luminance)
        target = float(np.median(L[m]))
        gain = np.clip(target / np.maximum(I, 1e-4), *clamp)
        out[m] = con[m] * gain[m][:, None]
        # painter's shadow tint: blurred colour where the painted light is weakest vs strongest
        Irgb = np.stack([masked_blur(con[..., c], m, sigma) for c in range(3)], -1)
        lo, hi = np.percentile(I[m], [10, 90])
        dark = Irgb[m & (I <= lo)].mean(0)
        lite = Irgb[m & (I >= hi)].mean(0)
        report[n] = {"median_lum": target, "light_range": float(hi / max(lo, 1e-4)),
                     "shadow_ratio_rgb": [float(v) for v in dark / np.maximum(lite, 1e-4)]}
    for x0, y0, x1, y1 in annotations:
        out[y0:y1, x0:x1] = 0.0
    Image.fromarray((lin_to_srgb(out) * 255 + 0.5).astype(np.uint8)).save(out_png)
    # ink: the darkest 2% of the whole panel, median colour (sRGB)
    L = lum(con)
    ink = np.median(con[L <= np.percentile(L, 2)], axis=0)
    return {"objects": report, "ink_srgb": [float(v) for v in lin_to_srgb(ink)]}
