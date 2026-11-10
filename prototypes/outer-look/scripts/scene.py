"""The painted-plate flight scene, composited offscreen: what the engine would draw.

    python -I scripts/scene.py <layers folder> <out folder> [--only flight|mine|dock] [--stills]

<layers> holds cut/ (from cut.py) and ship/ (from ship_frames.py). Writes JPG frames per
clip into <out>/<clip>/ and an MP4 per clip when imageio-ffmpeg is installed, plus stills.

Every layer here is something a small engine feature can draw in real time: textured
quads with alpha, additive glow quads, a per-sprite erosion mask, and a turn sheet picked
by heading. Nothing per frame is heavier than a sprite batch.

World: the play plane, in plate pixels at zoom 1. The camera looks down at 40 degrees, so
plane y is drawn foreshortened by sin(40). Headings are in degrees, 0 = screen right,
counter-clockwise.
"""
import math
import os
import sys

import cv2
import numpy as np

LAYERS, OUT = sys.argv[1], sys.argv[2]
ONLY = sys.argv[sys.argv.index("--only") + 1] if "--only" in sys.argv else None
W, H, FPS = 1280, 720, 30
FORE = math.sin(math.radians(40))
rng = np.random.default_rng(7)


def load(name):
    im = cv2.imread(os.path.join(LAYERS, "cut", name + ".png"), cv2.IMREAD_UNCHANGED)
    return im.astype(np.float32) / 255.0


def hexf(h):
    h = h.lstrip("#")
    return np.array([int(h[i:i + 2], 16) for i in (4, 2, 0)], np.float32) / 255.0  # BGR


# ---------------------------------------------------------------- sky (procedural layers)
def fbm(h, w, octaves, seed):
    r = np.random.default_rng(seed)
    acc = np.zeros((h, w), np.float32)
    amp, tot = 1.0, 0.0
    for o in range(octaves):
        s = 2 ** (o + 2)
        n = r.random((max(2, h * s // w), s)).astype(np.float32)
        acc += amp * cv2.resize(n, (w, h), interpolation=cv2.INTER_CUBIC)
        tot += amp
        amp *= 0.55
    return acc / tot


def make_sky(sw, sh):
    # colours sampled from the flight concept's empty sky (palette.py, see README)
    top, bot = hexf("#3b1f4c"), hexf("#2c163c")
    g = np.linspace(0, 1, sh, dtype=np.float32)[:, None, None]
    sky = top * (1 - g) + bot * g
    sky = np.broadcast_to(sky, (sh, sw, 3)).copy()
    for col, seed, amt in (("#a8306a", 1, 0.55), ("#2f7f8c", 2, 0.4), ("#6a2f8a", 3, 0.45)):
        n = fbm(sh, sw, 6, seed)
        m = np.clip((n - 0.56) * 3.2, 0, 1) ** 1.6 * amt
        sky = sky * (1 - m[..., None]) + hexf(col) * m[..., None]
    return np.clip(sky, 0, 1)


def make_stars(sw, sh, n, big, seed):
    r = np.random.default_rng(seed)
    lay = np.zeros((sh, sw, 3), np.float32)
    cols = [hexf(c) for c in ("#fff4dd", "#ffd8c2", "#cff3ee", "#ffe9a8")]
    for _ in range(n):
        x, y = int(r.integers(0, sw)), int(r.integers(0, sh))
        c = cols[int(r.integers(0, 4))] * float(r.uniform(0.35, 1.0))
        rad = 1 if r.random() > big else 2
        cv2.circle(lay, (x, y), rad - 1 if rad > 1 else 0, c.tolist(), -1, cv2.LINE_AA)
    return cv2.GaussianBlur(lay, (0, 0), 0.6)


SKY = make_sky(2600, 1600)
STARS = [make_stars(2600, 1600, 900, 0.97, 11), make_stars(2600, 1600, 260, 0.85, 12)]


def sky_frame(cx, cy, zoom):
    out = crop_wrap(SKY, cx * 0.05, -cy * FORE * 0.05)
    for lay, par in zip(STARS, (0.12, 0.3)):
        out = out + crop_wrap(lay, cx * par * zoom, -cy * FORE * par * zoom)
    return np.clip(out, 0, 1)


def crop_wrap(img, ox, oy):
    h, w = img.shape[:2]
    x0, y0 = int(ox) % w, int(oy) % h
    big = np.concatenate([img, img], 1)
    big = np.concatenate([big, big], 0)
    return big[y0:y0 + H, x0:x0 + W]


# ---------------------------------------------------------------- compositing helpers
def blit(dst, rgba, sx, sy, scale, angle=0.0, add=False, gain=1.0):
    """Draw a straight-alpha RGBA sprite centred at screen (sx, sy)."""
    h, w = rgba.shape[:2]
    m = cv2.getRotationMatrix2D((w / 2, h / 2), angle, scale)
    m[0, 2] += sx - w / 2
    m[1, 2] += sy - h / 2
    # only warp the screen box the sprite covers
    r = max(w, h) * scale * 0.75 + 4
    x0, y0 = max(0, int(sx - r)), max(0, int(sy - r))
    x1, y1 = min(W, int(sx + r)), min(H, int(sy + r))
    if x1 <= x0 or y1 <= y0:
        return
    m[0, 2] -= x0
    m[1, 2] -= y0
    warped = cv2.warpAffine(rgba, m, (x1 - x0, y1 - y0), flags=cv2.INTER_LINEAR,
                            borderMode=cv2.BORDER_CONSTANT, borderValue=(0, 0, 0, 0))
    sub = dst[y0:y1, x0:x1]
    if add:
        sub += warped[..., :3] * warped[..., 3:] * gain
    else:
        a = warped[..., 3:]
        sub[:] = sub * (1 - a) + warped[..., :3] * a


def glow_sprite(radius, col, falloff=2.2):
    s = int(radius * 2 + 2)
    y, x = np.mgrid[0:s, 0:s].astype(np.float32)
    d = np.sqrt((x - s / 2) ** 2 + (y - s / 2) ** 2) / radius
    a = np.clip(1 - d, 0, 1) ** falloff
    return np.dstack([np.broadcast_to(col, (s, s, 3)), a]).astype(np.float32)


# ---------------------------------------------------------------- station plate
STATION_SCALE = 0.5  # the plate is painted at 2x flight size: docking zooms to native paint
station = load("station")
sh_, sw_ = station.shape[:2]
hsv = cv2.cvtColor((station[..., :3] * 255).astype(np.uint8), cv2.COLOR_BGR2HSV)
warm = (hsv[..., 0] >= 12) & (hsv[..., 0] <= 32) & (hsv[..., 2] > 225) & (hsv[..., 1] > 80)
warm &= station[..., 3] > 0.9
warm = cv2.morphologyEx(warm.astype(np.uint8), cv2.MORPH_OPEN, np.ones((2, 2), np.uint8))
n_lamps, lamp_lab, lamp_stats, lamp_cent = cv2.connectedComponentsWithStats(warm)
lamp_phase = rng.uniform(0, 2 * math.pi, n_lamps)
lamp_rate = rng.uniform(0.6, 2.4, n_lamps)
lamp_kind = rng.random(n_lamps)  # a few lamps flicker, the rest breathe
WINDOW_GLOW = hexf("#ffb44a")
# the lit-window light: one blurred map per lamp would be costly, so lamps are grouped into
# 6 bands that share a phase; the engine would do the same with a glow-group channel
bands = 6
lamp_band = (np.arange(n_lamps) % bands)
band_masks = []
for b in range(bands):
    m = np.isin(lamp_lab, np.where(lamp_band == b)[0][1:] if b == 0 else np.where(lamp_band == b)[0]) & (lamp_lab > 0)
    m = m.astype(np.float32)
    band_masks.append(cv2.GaussianBlur(m, (0, 0), 7) * 1.6 + cv2.GaussianBlur(m, (0, 0), 2) * 0.6)
print("station lamps found:", n_lamps - 1)

# rooms on the plate: ellipses in plate pixels (centre x, y, radius x, y), the clickable parts
ROOMS = {
    "hall": (950, 430, 230, 190),
    "greenhouse": (1360, 520, 175, 140),
    "hangar": (520, 200, 220, 120),
    "galley": (1400, 225, 150, 110),
    "dock": (300, 640, 290, 110),
}


def room_hit(px, py):
    for name, (x, y, rx, ry) in ROOMS.items():
        if ((px - x) / rx) ** 2 + ((py - y) / ry) ** 2 <= 1:
            return name
    return None


def room_outline(name):
    """The hovered room's highlight: its painted pixels inside the ellipse, rim-inked."""
    x, y, rx, ry = ROOMS[name]
    m = np.zeros((sh_, sw_), np.uint8)
    cv2.ellipse(m, (x, y), (rx, ry), 0, 0, 360, 1, -1)
    m = (m > 0) & (station[..., 3] > 0.5)
    m = cv2.morphologyEx(m.astype(np.uint8), cv2.MORPH_OPEN, np.ones((5, 5), np.uint8))
    edge = cv2.dilate(m, np.ones((7, 7), np.uint8)) - m
    hl = np.zeros((sh_, sw_, 4), np.float32)
    hl[m > 0] = list(hexf("#ffd98a")) + [0.18]
    hl[edge > 0] = list(hexf("#fff1c8")) + [0.95]
    return hl


ROOM_HL = {}


def draw_station(dst, t, cam, hover=None):
    sx, sy, z = to_screen(STATION_POS, cam)
    blit(dst, station, sx, sy, z * STATION_SCALE)
    # lit windows and lamps: additive light, each band on its own phase
    light = np.zeros((sh_, sw_), np.float32)
    for b in range(bands):
        ph = lamp_phase[b] + t * lamp_rate[b]
        if b == 2:  # one band flickers like a failing tube
            k = 0.55 + 0.45 * (math.sin(t * 23.0) > -0.3) * (0.7 + 0.3 * math.sin(t * 7.1))
        else:
            k = 0.65 + 0.35 * math.sin(ph)
        light += band_masks[b] * k
    glow = np.dstack([np.broadcast_to(WINDOW_GLOW, (sh_, sw_, 3)), np.clip(light, 0, 1.5)])
    blit(dst, glow, sx, sy, z * STATION_SCALE, add=True, gain=0.55)
    # the beacon on the hall's crown blinks
    if (t % 1.6) < 0.25:
        bx, by = 985, 185
        blit(dst, glow_sprite(40, hexf("#ffcf6b")), sx + (bx - sw_ / 2) * z * STATION_SCALE,
             sy + (by - sh_ / 2) * z * STATION_SCALE, z, add=True, gain=1.3)
    if hover:
        if hover not in ROOM_HL:
            ROOM_HL[hover] = room_outline(hover)
        blit(dst, ROOM_HL[hover], sx, sy, z * STATION_SCALE)


# ---------------------------------------------------------------- rocks and mining
class Rock:
    def __init__(self, name, pos, scale, kind):
        self.img, self.pos, self.scale, self.kind = load(name), pos, scale, kind
        h, w = self.img.shape[:2]
        y, x = np.mgrid[0:h, 0:w].astype(np.float32)
        self.x, self.y = x, y
        self.noise = cv2.resize(rng.random((h // 9 + 2, w // 9 + 2)).astype(np.float32), (w, h),
                                interpolation=cv2.INTER_CUBIC) * 0.5 + \
            cv2.resize(rng.random((h // 4 + 2, w // 4 + 2)).astype(np.float32), (w, h)) * 0.2
        self.mined = 0.0
        self.bite_dir = (-1.0, 0.0)
        self.cache = None
        # cracks: random walks from the bite side, revealed as the rock is mined
        self.cracks = []
        for _ in range(7):
            pts = [(w * rng.uniform(0.2, 0.5), h * rng.uniform(0.3, 0.7))]
            ang = rng.uniform(-0.6, 0.6)
            for _ in range(int(rng.integers(5, 10))):
                ang += rng.uniform(-0.7, 0.7)
                L = rng.uniform(5, 14)
                pts.append((pts[-1][0] + math.cos(ang) * L, pts[-1][1] + math.sin(ang) * L))
            self.cracks.append((rng.uniform(0.1, 0.8), np.array(pts, np.int32)))

    def sprite(self):
        key = round(self.mined, 3)
        if self.cache and self.cache[0] == key:
            return self.cache[1]
        img = self.img.copy()
        h, w = img.shape[:2]
        a0 = img[..., 3]
        t = self.mined
        if t > 0:
            # the bite eats in from the side facing the ship, with a rough edge
            dx, dy = self.bite_dir
            proj = ((self.x - w / 2) * dx + (self.y - h / 2) * dy) / (0.5 * max(w, h))
            rough = proj + (self.noise - 0.35) * 0.55
            cut = rough > 1.1 - 1.3 * t
            keep = (a0 > 0.5) & ~cut
            keep = cv2.morphologyEx(keep.astype(np.uint8), cv2.MORPH_OPEN, np.ones((3, 3), np.uint8)) > 0
            # the fresh face: a warm band just inside the cut, inked at the cut
            inner = cv2.erode(keep.astype(np.uint8), np.ones((9, 9), np.uint8)) > 0
            cutside = cv2.dilate(cut.astype(np.uint8), np.ones((11, 11), np.uint8)) > 0
            face = keep & ~inner & cutside
            bite = hexf("#f2a35e") if self.kind != "ice" else hexf("#e8f6ff")
            img[face, :3] = img[face, :3] * 0.35 + bite * 0.65
            ink = keep & ~(cv2.erode(keep.astype(np.uint8), np.ones((3, 3), np.uint8)) > 0) & cutside
            img[ink, :3] = hexf("#2a1418")
            for start, pts in self.cracks:
                if t > start:
                    n = max(2, int(len(pts) * min(1, (t - start) * 3)))
                    lay = np.zeros((h, w), np.uint8)
                    cv2.polylines(lay, [pts[:n]], False, 1, 1, cv2.LINE_AA)
                    on = (lay > 0) & keep
                    img[on, :3] = img[on, :3] * 0.3 + hexf("#2a1418") * 0.7
            a = a0 * keep
            img[..., 3] = cv2.GaussianBlur(a.astype(np.float32), (3, 3), 0.7)
        self.cache = (key, img)
        return img

    def draw(self, dst, cam, t=0.0):
        sx, sy, z = to_screen(self.pos, cam)
        shrink = self.shrink()
        if self.mined < 0.95:
            blit(dst, self.sprite(), sx, sy, z * self.scale * shrink, angle=math.sin(t * 0.3 + self.pos[0]) * 2)
        if self.mined > 0.75:
            # the pebbles left at empty: small pieces of the same painting, warm-capped
            k = min(1, (self.mined - 0.75) * 4)
            for i, (ox, oy, s) in enumerate(((-30, 10, 0.16), (20, -14, 0.12), (36, 18, 0.1), (-6, -26, 0.09))):
                p = self.pebble(i)
                blit(dst, p, sx + ox * z * self.scale * 2, sy + oy * z * self.scale * 2, z * s * self.scale * 2.2 * k)

    def shrink(self):
        return 1.0 - 0.5 * self.mined

    def surface(self):
        """Where a beam along the sprite's middle row meets the rock, in plane coordinates."""
        img = self.sprite()
        h, w = img.shape[:2]
        row = img[h // 2 - 3:h // 2 + 4, :, 3].max(0)
        xs = np.nonzero(row > 0.5)[0]
        x = xs[0] if len(xs) else w / 2
        return (self.pos[0] + (x - w / 2) * self.scale * self.shrink() + 4, self.pos[1])

    def pebble(self, i):
        if not hasattr(self, "_pebbles"):
            h, w = self.img.shape[:2]
            self._pebbles = []
            for j in range(4):
                cx, cy = int(w * (0.35 + 0.1 * j)), int(h * (0.4 + 0.07 * j))
                r = int(min(w, h) * 0.22)
                p = self.img[max(0, cy - r):cy + r, max(0, cx - r):cx + r].copy()
                m = np.zeros(p.shape[:2], np.uint8)
                pts = np.array([[r + int(math.cos(a) * r * rng.uniform(0.6, 0.95)),
                                 r + int(math.sin(a) * r * rng.uniform(0.6, 0.95))]
                                for a in np.linspace(0, 2 * math.pi, 7)[:-1]], np.int32)
                cv2.fillPoly(m, [pts], 1)
                cap = (m > 0) & (np.mgrid[0:p.shape[0], 0:p.shape[1]][0] < r * 0.8)
                p[..., 3] = p[..., 3] * m
                p[cap, :3] = p[cap, :3] * 0.6 + hexf("#f2a35e") * 0.4
                edge = (cv2.dilate(m, np.ones((5, 5), np.uint8)) - m) > 0
                p[edge] = list(hexf("#2a1418")) + [1.0]
                self._pebbles.append(p)
        return self._pebbles[i]


# ---------------------------------------------------------------- the ship
def load_turn(variant):
    d = os.path.join(LAYERS, "paint", "out") if variant == "painted" else os.path.join(LAYERS, "ship", variant)
    frames = sorted(f for f in os.listdir(d) if f.endswith(".png"))
    return [cv2.imread(os.path.join(d, f), cv2.IMREAD_UNCHANGED).astype(np.float32) / 255 for f in frames]


SHIP = {"base": load_turn("base"), "pod": load_turn("pod")}
# --ship painted: the 16 painted-over frames (paint/out/paint_NN.png, 512 px) instead of the
# 64 rendered ones; headings between two frames turn the nearest frame in the screen plane
SHIP_VARIANT = sys.argv[sys.argv.index("--ship") + 1] if "--ship" in sys.argv else None
if SHIP_VARIANT == "painted":
    SHIP["painted"] = load_turn("painted")
SHIP_SCALE = 0.62  # turn-sheet pixels to plate pixels


def draw_ship(dst, cam, pos, heading, thrust, t, variant="base"):
    frames = SHIP[variant]
    n = len(frames)
    sx, sy, z = to_screen(pos, cam)
    i = int(round(heading / 360 * n)) % n
    residual = heading - i * 360 / n
    residual = (residual + 180) % 360 - 180
    px_scale = 256 / frames[0].shape[1]
    # engine glow first (behind the hull on most headings): an additive cone at the tail
    hr = math.radians(heading)
    tail = (-math.cos(hr) * 60, math.sin(hr) * 60 * FORE)
    if thrust > 0.02:
        L = (0.6 + 0.4 * thrust) * (0.9 + 0.1 * math.sin(t * 40))
        for k, (r, gcol, g) in enumerate(((26, "#ff8a3a", 0.9), (14, "#ffe0a0", 1.2))):
            gs = glow_sprite(r * L, hexf(gcol))
            for step in range(4):
                f = step / 3
                blit(dst, gs, sx + tail[0] * z * (1 + f * L * 0.8), sy + tail[1] * z * (1 + f * L * 0.8),
                     z * (1 - f * 0.6), add=True, gain=g * (1 - f * 0.7) * thrust)
    blit(dst, frames[i], sx, sy, z * SHIP_SCALE * px_scale, angle=residual if n < 64 else 0.0)
    return sx, sy, z


def beam(dst, a, b, t):
    """The mining beam: a warm additive line with a soft core and a hot spot where it lands."""
    lay = np.zeros((H, W), np.float32)
    cv2.line(lay, (int(a[0]), int(a[1])), (int(b[0]), int(b[1])), 1.0, 3, cv2.LINE_AA)
    core = lay.copy()
    lay = cv2.GaussianBlur(lay, (0, 0), 5) * 2.2
    wob = 0.85 + 0.15 * math.sin(t * 31)
    dst += (lay[..., None] * hexf("#ff9a4a") + core[..., None] * hexf("#fff0c0")) * wob
    blit(dst, glow_sprite(34, hexf("#ffb060")), b[0], b[1], 1.0, add=True, gain=1.4 * wob)


SPARKS = []


def sparks(dst, at, t, dt, rate=40):
    for _ in range(int(rate * dt + rng.random())):
        ang = rng.uniform(0, 2 * math.pi)
        sp = rng.uniform(60, 220)
        SPARKS.append([at[0], at[1], math.cos(ang) * sp, math.sin(ang) * sp - 40, rng.uniform(0.3, 0.8)])
    for s in SPARKS:
        s[0] += s[2] * dt
        s[1] += s[3] * dt
        s[3] += 220 * dt
        s[4] -= dt
        if s[4] > 0:
            cv2.circle(dst, (int(s[0]), int(s[1])), 1, (hexf("#ffd27a") * min(1, s[4] * 2)).tolist(), -1, cv2.LINE_AA)
    SPARKS[:] = [s for s in SPARKS if s[4] > 0]


# ---------------------------------------------------------------- camera
STATION_POS = (0.0, 0.0)


def to_screen(p, cam):
    cx, cy, z = cam
    return (p[0] - cx) * z + W / 2, -(p[1] - cy) * FORE * z + H / 2, z


ROCKS = [
    Rock("rock_iron_big", (980, 330), 0.8, "iron"),
    Rock("rock_ice", (1180, -60), 0.7, "ice"),
    Rock("rock_glassite", (1350, -420), 0.7, "glassite"),
    Rock("rock_iron_mid", (760, -330), 0.8, "iron"),
    Rock("rock_iron_small", (1500, 260), 0.7, "iron"),
    Rock("rock_iron_low", (-700, -700), 0.8, "iron"),
    Rock("rock_iron_flat", (300, -760), 0.8, "iron"),
    Rock("rock_iron_long", (1650, -900), 0.75, "iron"),
    Rock("rock_iron_tiny", (1080, 120), 0.8, "iron"),
]


def render(dst, t, cam, ship=None, hover=None, beam_to=None, dt=1 / FPS):
    dst[:] = sky_frame(cam[0], cam[1], cam[2])
    items = [(STATION_POS[1] + 40, "station")] + [(r.pos[1], r) for r in ROCKS]
    if ship:
        items.append((ship["pos"][1], "ship"))
    items.sort(key=lambda it: -it[0])  # far (north) first
    for _, it in items:
        if it == "station":
            draw_station(dst, t, cam, hover)
        elif it == "ship":
            draw_ship(dst, cam, ship["pos"], ship["heading"], ship["thrust"], t, ship.get("variant", "base"))
        else:
            it.draw(dst, cam, t)
    if beam_to is not None and ship:
        hr = math.radians(ship["heading"])
        nose = (ship["pos"][0] + math.cos(hr) * 58, ship["pos"][1] + math.sin(hr) * 58)
        a = to_screen(nose, cam)
        b = to_screen(beam_to, cam)
        beam(dst, a, b, t)
        sparks(dst, b, t, dt)


def save(frame, path):
    cv2.imwrite(path, (np.clip(frame, 0, 1) * 255).astype(np.uint8), [cv2.IMWRITE_JPEG_QUALITY, 90])


def encode(folder, mp4):
    try:
        import imageio_ffmpeg
    except ImportError:
        print("no imageio-ffmpeg: frames only in", folder)
        return
    import subprocess
    subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-y", "-loglevel", "error", "-framerate", str(FPS),
                    "-i", os.path.join(folder, "%05d.jpg"), "-c:v", "libx264", "-pix_fmt", "yuv420p",
                    "-crf", "22", "-movflags", "+faststart", mp4], check=True)
    print("wrote", mp4)


def ease(x):
    x = min(1, max(0, x))
    return x * x * (3 - 2 * x)


# ---------------------------------------------------------------- the clips
def clip_flight():
    """The ship comes in from the right, loops once in front of the station, flies past it."""
    folder = os.path.join(OUT, "flight")
    os.makedirs(folder, exist_ok=True)
    frame = np.zeros((H, W, 3), np.float32)
    pos = [900.0, -380.0]
    heading = 180.0
    cam = [pos[0], pos[1], 1.0]
    n = 8 * FPS
    for f in range(n):
        t = f / FPS
        dt = 1 / FPS
        turn = 0.0
        if 1.4 < t <= 3.4:
            turn = 180.0  # a full loop at the chosen 180 degrees a second
        elif 4.6 < t <= 5.1:
            turn = -180.0  # bank right, up past the dock
        elif 6.0 < t <= 6.5:
            turn = 180.0
        heading = (heading + turn * dt) % 360
        sp = 240.0
        hr = math.radians(heading)
        pos[0] += math.cos(hr) * sp * dt
        pos[1] += math.sin(hr) * sp * dt
        cam[0] += (pos[0] - cam[0]) * 0.08
        cam[1] += (pos[1] - cam[1]) * 0.08
        cam[2] = 0.95
        render(frame, t, cam, ship={"pos": pos, "heading": heading, "thrust": 1.0, "variant": SHIP_VARIANT or "base"})
        save(frame, os.path.join(folder, "%05d.jpg" % f))
    encode(folder, os.path.join(OUT, "flight.mp4"))


def clip_mine():
    """The ship turns to an iron rock, fits its beam and mines it down to pebbles."""
    folder = os.path.join(OUT, "mine")
    os.makedirs(folder, exist_ok=True)
    frame = np.zeros((H, W, 3), np.float32)
    rock = ROCKS[0]
    rock.mined = 0.0
    pos = [rock.pos[0] - 260, rock.pos[1] - 40]
    target_h = math.degrees(math.atan2(rock.pos[1] - pos[1], rock.pos[0] - pos[0]))
    heading = target_h - 120
    n = 7 * FPS
    for f in range(n):
        t = f / FPS
        heading += max(-6.0, min(6.0, (target_h - heading))) if t < 1.2 else 0
        mining = 1.2 < t < 6.2
        rock.mined = ease((t - 1.3) / 4.8) if t > 1.3 else 0.0
        rock.bite_dir = (-1.0, 0.25)
        cam = (pos[0] + 150, pos[1] + 10, 1.55)
        # the beam lands on the rock's current surface: walk from the ship toward the rock's centre
        hit = None
        if mining:
            hit = rock.surface() if rock.mined < 0.9 else (rock.pos[0] - 40, rock.pos[1])
        render(frame, t, cam, ship={"pos": pos, "heading": heading, "thrust": 0.15, "variant": SHIP_VARIANT or "pod"},
               beam_to=hit)
        save(frame, os.path.join(folder, "%05d.jpg" % f))
    encode(folder, os.path.join(OUT, "mine.mp4"))


def clip_dock():
    """The camera zooms from flight onto the station, as docking does; rooms light on hover."""
    folder = os.path.join(OUT, "dock")
    os.makedirs(folder, exist_ok=True)
    frame = np.zeros((H, W, 3), np.float32)
    n = 6 * FPS
    order = ["dock", "hall", "dock", "hall"]
    for f in range(n):
        t = f / FPS
        k = ease(t / 3.2)
        z = 0.75 + (2.0 - 0.75) * k
        # zoom onto the dock end of the plate (plate px 300, 640 -> plane coordinates)
        tx = (300 - sw_ / 2) * STATION_SCALE
        ty = -(640 - sh_ / 2) * STATION_SCALE / FORE
        cam = (tx * k + 200 * (1 - k), ty * k, z)
        hover = order[min(len(order) - 1, int((t - 3.3) / 0.7))] if t > 3.3 else None
        render(frame, t, cam, hover=hover)
        save(frame, os.path.join(folder, "%05d.jpg" % f))
    encode(folder, os.path.join(OUT, "dock.mp4"))


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in (("flight", clip_flight), ("mine", clip_mine), ("dock", clip_dock)):
        if ONLY in (None, name):
            fn()
