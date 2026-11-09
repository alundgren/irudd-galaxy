"""Cut the concept's porthole view into depth layers (sky+planet, station tower, left platform).

usage: python -I porthole_layers.py <room.png> <outdir>
Writes: window-mask.png, struct-mask.png, layers-preview.png, sky-plate.png (structures inpainted),
        layer-tower.png, layer-left.png (RGBA cut-outs), layers.json (pixel boxes).
Segmentation: inside the window ellipse, sky and planet are saturated violet/pink; station structures
are desaturated lavender/grey with warm window lights. Cleaned with a small open/close and by
keeping large connected parts only.
"""
import json, os, sys
import numpy as np
from PIL import Image, ImageFilter

src, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
im = Image.open(src).convert("RGB")
a = np.asarray(im).astype(np.float32) / 255
H, W = a.shape[:2]
yy, xx = np.mgrid[0:H, 0:W]

# window opening (glass) ellipse, concept crop pixels, read off concept/porthole-crop.png
CX, CY, RX, RY = 127, 303, 108, 158
win = ((xx - CX) / RX) ** 2 + ((yy - CY) / RY) ** 2 < 1.0

def morph(m, size, op):
    img = Image.fromarray((m * 255).astype(np.uint8))
    f = ImageFilter.MinFilter(size) if op == "erode" else ImageFilter.MaxFilter(size)
    return np.asarray(img.filter(f)) > 127


# Colour segmentation was tried first and failed (it caught only lit tops; shadowed structure shares
# the sky's violet). Layers are cut by hand like a matte painter would: polygons traced on
# concept/porthole-grid.png, in room.png pixels.
TOWER = [(133, 277), (160, 268), (197, 266), (198, 195), (215, 188), (242, 200), (243, 462), (178, 462),
         (175, 357), (128, 353), (119, 340), (140, 320), (172, 303), (190, 293), (145, 291)]
LEFT = [(13, 312), (25, 309), (37, 312), (38, 345), (70, 343), (110, 350), (140, 372), (135, 386), (90, 393),
        (83, 398), (82, 412), (55, 412), (52, 398), (13, 395)]
PLANT = [(0, 353), (40, 390), (85, 405), (122, 440), (128, 480), (0, 480)]   # room object in front of the window


def poly_mask(pts):
    from PIL import ImageDraw
    img = Image.new("L", (W, H), 0)
    ImageDraw.Draw(img).polygon(pts, fill=255)
    return np.asarray(img) > 127


tower = poly_mask(TOWER) & win
left = poly_mask(LEFT) & win
plant = poly_mask(PLANT)
struct = tower | left

# sky plate: structures removed by push-pull fill from the remaining window pixels
w = (win & ~morph(struct | plant, 5, "dilate")).astype(np.float32)


def push_pull(rgb, w):
    levels = [(rgb * w[..., None], w)]
    while min(levels[-1][1].shape) > 1:
        c, ww = levels[-1]
        h, wd = ww.shape[0] // 2, ww.shape[1] // 2
        levels.append((c[:h * 2, :wd * 2].reshape(h, 2, wd, 2, 3).mean((1, 3)),
                       ww[:h * 2, :wd * 2].reshape(h, 2, wd, 2).mean((1, 3))))
    c, ww = levels[-1]
    val = c / np.maximum(ww, 1e-6)[..., None]
    for c, ww in reversed(levels[:-1]):
        up = np.repeat(np.repeat(val, 2, 0), 2, 1)
        up = np.pad(up, ((0, ww.shape[0] - up.shape[0]), (0, ww.shape[1] - up.shape[1]), (0, 0)), mode="edge")
        up = np.asarray(Image.fromarray((np.clip(up, 0, 1) * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(1))) / 255.0
        al = np.clip(ww * 4, 0, 1)[..., None]
        val = al * (c / np.maximum(ww, 1e-6)[..., None]) + (1 - al) * up
    return val


PAD = 800   # the sky is seen far beyond the concept frame from other angles: inpaint a padded canvas
ap = np.zeros((H + 2 * PAD, W + 2 * PAD, 3), np.float32)
wp = np.zeros((H + 2 * PAD, W + 2 * PAD), np.float32)
ap[PAD:PAD + H, PAD:PAD + W] = a
wp[PAD:PAD + H, PAD:PAD + W] = w
# colour where the painter showed no sky: smooth continuation near the paint, fading with distance to
# the painter's deep sky colour (median of the darker quarter of painted sky pixels, planet excluded:
# a per-row profile was tried and smeared the planet's pink into a band across the whole sky)
skyonly = (w > 0.5) & (a[..., 2] > a[..., 0] * 1.05)
Ls = (a @ np.array([0.2126, 0.7152, 0.0722], np.float32))[skyonly]
deep = np.median(a[skyonly][Ls <= np.percentile(Ls, 25)], axis=0)
pp = push_pull(ap, wp)
dist_w = np.asarray(Image.fromarray((wp * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(60))) / 255.0
near = np.clip(dist_w * 2.5, 0, 1)[..., None]
sky = near * pp + (1 - near) * deep
known = wp > 0.5
sky = np.where(known[..., None], ap, sky)

# stars in the invented sky at the density the painter used: star pixels = clearly brighter than their
# 6 px surroundings inside the seen sky; sprinkle the same share of pixels, same brightness range
L = a @ np.array([0.2126, 0.7152, 0.0722], np.float32)
Lb = np.asarray(Image.fromarray((L * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(6))) / 255.0
seen_sky = w > 0.5
excess = (L - Lb)[seen_sky]
STAR_T = 0.25   # a star is much brighter than its surroundings; lower thresholds count paint texture
star_share = float((excess > STAR_T).mean())
star_gain = excess[excess > STAR_T] if (excess > STAR_T).any() else np.array([0.3])
rng = np.random.default_rng(3)
n_stars = int(star_share * (~known).sum() / 6)      # a painted star covers ~6 px
ys = rng.integers(0, sky.shape[0], n_stars)
xs = rng.integers(0, sky.shape[1], n_stars)
keep = ~known[ys, xs]
star = np.zeros(sky.shape[:2], np.float32)
star[ys[keep], xs[keep]] = rng.choice(star_gain, keep.sum()) * 4
star = np.asarray(Image.fromarray(np.clip(star * 255, 0, 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.7))) / 255.0
sky = sky + star[..., None] * np.array([0.95, 0.9, 1.0])
Image.fromarray((np.clip(sky, 0, 1) * 255).astype(np.uint8)).save(os.path.join(out, "sky-plate.png"))
STARS = {"star_share": star_share, "stars_added": int(keep.sum()), "pad": PAD}


def rgba(mask, name):
    m = np.asarray(Image.fromarray((mask * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.8)))
    Image.fromarray(np.dstack([(a * 255).astype(np.uint8), m])).save(os.path.join(out, name))


rgba(tower, "layer-tower.png")
rgba(left, "layer-left.png")
Image.fromarray((win * 255).astype(np.uint8)).save(os.path.join(out, "window-mask.png"))
prev = (a * 0.35 * 255).astype(np.uint8)
prev[win] = (a[win] * 255).astype(np.uint8)
prev[tower] = (prev[tower] * 0.5 + np.array([0, 255, 255]) * 0.5).astype(np.uint8)
prev[left] = (prev[left] * 0.5 + np.array([255, 255, 0]) * 0.5).astype(np.uint8)
crop = (0, 100, 260, 480)
pv = Image.fromarray(prev).crop(crop)
sk = Image.open(os.path.join(out, "sky-plate.png")).crop((crop[0] + PAD, crop[1] + PAD, crop[2] + PAD, crop[3] + PAD))
s = Image.new("RGB", (520, 380))
s.paste(pv, (0, 0))
s.paste(sk, (260, 0))
s.resize((1040, 760)).save(os.path.join(out, "layers-preview.png"))


def box(m):
    ys, xs = np.nonzero(m)
    return [int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max())] if len(xs) else None


json.dump({**STARS, "window": [CX, CY, RX, RY], "tower": box(tower), "left": box(left),
           "tower_px": int(tower.sum()), "left_px": int(left.sum())}, open(os.path.join(out, "layers.json"), "w"), indent=1)
print(open(os.path.join(out, "layers.json")).read())
