"""A region built only from NEW generated art: three stations, a rock field, two ships.

    python -I scripts/region.py <work> <out folder>

<work> holds (all made by the image tool from text briefs, untouched):
  stations/station_<f>.png and stations/roommap_<f>.png for f in wharf, tollway, lamplighter
  rocks/rocks_<kind>.png (three rocks per sheet) for kind in iron, ice, glassite
  paint/out/paint_NN.png (the tug) and paint2/out/paint_NN.png (the hauler), 16 views each
Room click-shapes come from the room maps, snapped to their seven colours; lamps are found
in the paintings. Nothing is placed or traced by hand except where things sit in the region.

Writes <out>/region/NNNNN.jpg, <out>/region.mp4 and <out>/region-stills.jpg.
"""
import math
import os
import sys

import cv2
import numpy as np

WORK, OUT = sys.argv[1], sys.argv[2]
sys.argv = [sys.argv[0], WORK, OUT, "--ship", "painted"]
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scene as S  # noqa: E402  (the compositor: sky, blit, glow, rocks, beam, ships)

H, W, FPS = S.H, S.W, S.FPS
ROOM_COLOURS = {"dock": (0, 0, 255), "office": (0, 255, 0), "galley": (255, 0, 0), "workshop": (0, 255, 255),
                "greenhouse": (255, 0, 255), "structure": (128, 128, 128), "bg": (0, 0, 0)}


class Station:
    def __init__(self, faction, pos, scale=0.5):
        self.pos, self.scale = pos, scale
        img = cv2.imread(os.path.join(WORK, "stations", "station_%s.png" % faction), cv2.IMREAD_UNCHANGED)
        self.img = img.astype(np.float32) / 255
        h, w = img.shape[:2]
        # rooms: snap the generated room map to its seven colours, clean it, keep rooms only
        mp = cv2.imread(os.path.join(WORK, "stations", "roommap_%s.png" % faction))
        mp = cv2.resize(mp, (w, h), interpolation=cv2.INTER_NEAREST).astype(np.float32)
        names = list(ROOM_COLOURS)
        pal = np.array([ROOM_COLOURS[n] for n in names], np.float32)
        lab = ((mp[..., None, :] - pal) ** 2).sum(-1).argmin(-1).astype(np.uint8)
        self.rooms = cv2.medianBlur(lab, 5)
        self.room_names = names
        self.hl = {}
        # lamps: warm bright pixels, grouped into six phase bands
        hsv = cv2.cvtColor(img[..., :3], cv2.COLOR_BGR2HSV)
        warm = (hsv[..., 0] >= 12) & (hsv[..., 0] <= 32) & (hsv[..., 2] > 225) & (hsv[..., 1] > 80) & (img[..., 3] > 230)
        warm = cv2.morphologyEx(warm.astype(np.uint8), cv2.MORPH_OPEN, np.ones((2, 2), np.uint8))
        n, lab2, _, _ = cv2.connectedComponentsWithStats(warm)
        self.bands = []
        for b in range(6):
            m = ((lab2 % 6) == b) & (lab2 > 0)
            m = m.astype(np.float32)
            self.bands.append(cv2.GaussianBlur(m, (0, 0), 7) * 1.6 + cv2.GaussianBlur(m, (0, 0), 2) * 0.6)
        self.lamps = n - 1
        self.phase = np.random.default_rng(len(faction)).uniform(0, 6.3, 6)

    def highlight(self, room):
        if room not in self.hl:
            m = (self.rooms == self.room_names.index(room)).astype(np.uint8)
            m = cv2.morphologyEx(m, cv2.MORPH_OPEN, np.ones((7, 7), np.uint8))
            edge = cv2.dilate(m, np.ones((9, 9), np.uint8)) - m
            hl = np.zeros(self.img.shape, np.float32)
            hl[m > 0] = list(S.hexf("#ffd98a")) + [0.2]
            hl[edge > 0] = list(S.hexf("#fff1c8")) + [0.95]
            self.hl[room] = hl
        return self.hl[room]

    def room_at(self, sx, sy, cam):
        """Which room is under a screen point: the click test."""
        cx, cy, z = S.to_screen(self.pos, cam)
        h, w = self.rooms.shape
        px = int((sx - cx) / (z * self.scale) + w / 2)
        py = int((sy - cy) / (z * self.scale) + h / 2)
        if 0 <= px < w and 0 <= py < h:
            name = self.room_names[self.rooms[py, px]]
            return name if name not in ("structure", "bg") else None
        return None

    def draw(self, dst, t, cam, hover=None):
        sx, sy, z = S.to_screen(self.pos, cam)
        if sx < -1200 or sx > W + 1200 or sy < -900 or sy > H + 900:
            return
        S.blit(dst, self.img, sx, sy, z * self.scale)
        light = np.zeros(self.img.shape[:2], np.float32)
        for b in range(6):
            k = (0.55 + 0.45 * (math.sin(t * 23) > -0.3)) if b == 2 else 0.65 + 0.35 * math.sin(self.phase[b] + t * (0.8 + b * 0.3))
            light += self.bands[b] * k
        glow = np.dstack([np.broadcast_to(S.WINDOW_GLOW, light.shape + (3,)), np.clip(light, 0, 1.5)])
        S.blit(dst, glow, sx, sy, z * self.scale, add=True, gain=0.55)
        if hover:
            S.blit(dst, self.highlight(hover), sx, sy, z * self.scale)


def split_rocks(kind):
    """Cut a sheet of three generated rocks into sprites, saved where scene.Rock loads them."""
    sheet = cv2.imread(os.path.join(WORK, "rocks", "rocks_%s.png" % kind), cv2.IMREAD_UNCHANGED)
    a = (sheet[..., 3] > 40).astype(np.uint8)
    n, lab, stats, _ = cv2.connectedComponentsWithStats(cv2.dilate(a, np.ones((5, 5), np.uint8)))
    order = sorted(range(1, n), key=lambda i: -stats[i, cv2.CC_STAT_AREA])[:3]
    names = []
    for k, i in enumerate(sorted(order, key=lambda i: stats[i, cv2.CC_STAT_LEFT])):
        x, y, w, h = stats[i, :4]
        sub = sheet[y:y + h, x:x + w].copy()
        sub[..., 3] = sub[..., 3] * (lab[y:y + h, x:x + w] == i)
        name = "new_%s_%d" % (kind, k)
        cv2.imwrite(os.path.join(WORK, "cut", name + ".png"), sub)
        names.append(name)
    return names


ROCKNAMES = {k: split_rocks(k) for k in ("iron", "ice", "glassite")}
S.SHIP["hauler"] = [cv2.imread(os.path.join(WORK, "paint2", "out", "paint_%02d.png" % i), cv2.IMREAD_UNCHANGED)
                    .astype(np.float32) / 255 for i in range(16)]

# the region: three stations along a lane, rock fields between them
STATIONS = [Station("wharf", (0, 0)), Station("tollway", (2600, 500)), Station("lamplighter", (5200, -200))]
rng = np.random.default_rng(3)
ROCKS = []
for cx, cy, kinds in ((1300, -350, ("iron", "iron", "ice")), (3900, -500, ("glassite", "iron", "ice")),
                      (1500, 900, ("ice", "glassite")), (4100, 800, ("iron", "glassite"))):
    for j in range(7):
        kind = kinds[j % len(kinds)]
        name = ROCKNAMES[kind][int(rng.integers(0, 3))]
        r = S.Rock(name, (cx + rng.uniform(-420, 420), cy + rng.uniform(-260, 260)), 1.0, kind)
        r.scale = float(rng.uniform(110, 230)) / r.img.shape[1]  # on-screen width in plane units at zoom 1
        ROCKS.append(r)
S.ROCKS[:] = ROCKS
print("stations: lamps", [s.lamps for s in STATIONS], "rocks", len(ROCKS))


def render(dst, t, cam, ships, hover=None, beam=None):
    dst[:] = S.sky_frame(cam[0], cam[1], cam[2])
    items = [(s.pos[1] + 60, "st", s) for s in STATIONS] + [(r.pos[1], "rock", r) for r in ROCKS]
    items += [(sh["pos"][1], "ship", sh) for sh in ships]
    items.sort(key=lambda it: -it[0])
    for _, kind, it in items:
        if kind == "st":
            it.draw(dst, t, cam, hover[1] if hover and hover[0] is it else None)
        elif kind == "rock":
            sx, sy, _ = S.to_screen(it.pos, cam)
            if -300 < sx < W + 300 and -300 < sy < H + 300:
                it.draw(dst, cam, t)
        else:
            S.draw_ship(dst, cam, it["pos"], it["heading"], it["thrust"], t, it["variant"])
    if beam:
        sh, hit = beam
        hr = math.radians(sh["heading"])
        a = S.to_screen((sh["pos"][0] + math.cos(hr) * 58, sh["pos"][1] + math.sin(hr) * 58), cam)
        b = S.to_screen(hit, cam)
        S.beam(dst, a, b, t)
        S.sparks(dst, b, t, 1 / FPS)


def steer(sh, target, dt, speed=240.0, turn=180.0):
    want = math.degrees(math.atan2(target[1] - sh["pos"][1], target[0] - sh["pos"][0]))
    d = (want - sh["heading"] + 180) % 360 - 180
    sh["heading"] = (sh["heading"] + max(-turn * dt, min(turn * dt, d))) % 360
    hr = math.radians(sh["heading"])
    sh["pos"][0] += math.cos(hr) * speed * dt
    sh["pos"][1] += math.sin(hr) * speed * dt
    return math.hypot(target[0] - sh["pos"][0], target[1] - sh["pos"][1])


def main():
    folder = os.path.join(OUT, "region")
    os.makedirs(folder, exist_ok=True)
    frame = np.zeros((H, W, 3), np.float32)
    tug = {"pos": [-200.0, -700.0], "heading": 0.0, "thrust": 1.0, "variant": "painted"}
    hauler = {"pos": [5600.0, -900.0], "heading": 180.0, "thrust": 1.0, "variant": "hauler"}
    route = [(1000, -650), (2000, -300), (2700, -250), (3400, -600)]
    hauler_route = [(4300, -900), (2600, -900), (800, -800), (-600, -600)]
    wp, hwp = 0, 0
    mine_rock = None
    mine_start = None
    mine_end = None
    cam = [tug["pos"][0], tug["pos"][1], 0.8]
    stills = []
    n = 40 * FPS
    for f in range(n):
        t = f / FPS
        dt = 1 / FPS
        if hwp < len(hauler_route) and steer(hauler, hauler_route[hwp], dt, speed=170) < 80:
            hwp += 1
        beam = None
        hover = None
        if t < 11:  # fly the lane past the wharf and the tollway into the far field
            if wp < len(route) and steer(tug, route[wp], dt) < 90:
                wp += 1
            tug["thrust"] = 1.0
            focus = tug["pos"]
            z = 0.8
        elif mine_end is None or t < mine_end:  # fly to the nearest rock and mine it down
            if mine_rock is None:
                mine_rock = min(ROCKS, key=lambda r: math.hypot(r.pos[0] - tug["pos"][0], r.pos[1] - tug["pos"][1]))
            half = mine_rock.img.shape[1] * mine_rock.scale / 2
            target = (mine_rock.pos[0] - half - 90, mine_rock.pos[1])
            if mine_start is None:
                if steer(tug, target, dt) < 25:
                    mine_start = t
                    mine_end = t + 4.0
            else:
                want = math.degrees(math.atan2(mine_rock.pos[1] - tug["pos"][1], mine_rock.pos[0] - tug["pos"][0]))
                tug["heading"] += ((want - tug["heading"] + 180) % 360 - 180) * 0.2
            tug["thrust"] = 0.15 if mine_start else 1.0
            if mine_start is not None and t > mine_start + 0.4:
                mine_rock.bite_dir = (-1.0, 0.0)
                mine_rock.mined = S.ease((t - mine_start - 0.4) / 3.0)
                hit = mine_rock.surface() if mine_rock.mined < 0.9 else (mine_rock.pos[0] - 20, mine_rock.pos[1])
                beam = (tug, hit)
            focus = (tug["pos"][0] + 110, tug["pos"][1])
            z = 1.6
        else:  # zoom in to dock at the tollway; rooms light as the pointer moves over them
            st = STATIONS[1]
            k = S.ease((t - mine_end) / 3.5)
            focus = (cam[0] + (st.pos[0] - cam[0]) * 0.1, cam[1] + (st.pos[1] - cam[1]) * 0.1)
            z = 0.8 + 1.0 * k
            if t > mine_end + 3.5:
                # a pointer sweeps across the plate; the room under it comes from the room map
                u = min(1.0, (t - mine_end - 3.5) / 5.5)
                px, py = W * (0.25 + 0.5 * u), H * (0.5 + 0.12 * math.sin(u * 9))
                name = st.room_at(px, py, (cam[0], cam[1], cam[2]))
                hover = (st, name) if name else None
        cam[0] += (focus[0] - cam[0]) * 0.08
        cam[1] += (focus[1] - cam[1]) * 0.08
        cam[2] += (z - cam[2]) * 0.08
        render(frame, t, tuple(cam), [tug, hauler], hover=hover, beam=beam)
        if hover:
            cv2.circle(frame, (int(px), int(py)), 7, (1, 1, 1), 2, cv2.LINE_AA)
        S.save(frame, os.path.join(folder, "%05d.jpg" % f))
        if mine_end is not None and t > mine_end + 9.0:
            break
        if f in (60, 200, 400) or (mine_start is not None and f in (int((mine_start + 1.2) * FPS), int((mine_start + 3.6) * FPS))) or (mine_end is not None and f == int((mine_end + 6.5) * FPS)):
            stills.append(cv2.resize((np.clip(frame, 0, 1) * 255).astype(np.uint8), (640, 360)))
    S.encode(folder, os.path.join(OUT, "region.mp4"))
    stills = (stills + [np.zeros((360, 640, 3), np.uint8)] * 6)[:6]
    cv2.imwrite(os.path.join(OUT, "region-stills.jpg"),
                np.vstack([np.hstack(stills[:3]), np.hstack(stills[3:])]), [cv2.IMWRITE_JPEG_QUALITY, 86])


if __name__ == "__main__":
    main()
