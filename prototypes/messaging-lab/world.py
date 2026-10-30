"""The little system: three stations, the Rimewater ice field, a buoy line, the lantern current and some plain rocks."""
import math
import random

from pygame.math import Vector2 as V

DOCK_R = 80  # fly inside this to dock
CELL = 512

# id, name, position
STATIONS = {
    "wharf": ("Lantern Wharf", V(0, 0)),
    "kettle": ("Kettle Dock", V(0, 4200)),
    "hook": ("Cinder Hook", V(6400, -1700)),
}
STATION_IDS = ("wharf", "kettle", "hook")

ICE_FIELD = V(1150, 320)  # Rimewater: pale ice rocks just east of the Wharf
ICE_FIELD_R = 480
MINE_RANGE = 130  # from the ship to a rock's edge

# blue buoys from the Wharf straight down to Kettle Dock
BUOYS = [V(0, y) for y in range(420, 3900, 420)]

# the lantern current: a slow stream of drifting lights from east of the Wharf out to Cinder Hook
CURRENT = [V(380, -260), V(1400, -620), V(2600, -820), V(3800, -1180), V(5000, -1460), V(5980, -1640)]
CURRENT_HALF_W = 150
CURRENT_SPEED = 110  # px/s of drift while inside it


def name(sid):
    return STATIONS[sid][0]


def pos(sid):
    return STATIONS[sid][1]


def _seg_lengths():
    return [(b - a).length() for a, b in zip(CURRENT, CURRENT[1:])]


SEG_LEN = _seg_lengths()
CURRENT_LEN = sum(SEG_LEN)


def current_at(s):
    """Point and unit tangent at distance s along the current."""
    s = max(0.0, min(CURRENT_LEN, s))
    for (a, b), L in zip(zip(CURRENT, CURRENT[1:]), SEG_LEN):
        if s <= L:
            t = (b - a) / L
            return a + t * s, t
        s -= L
    a, b = CURRENT[-2], CURRENT[-1]
    return V(b), (b - a).normalize()


def current_drift(p):
    """Drift velocity at p: along the current inside it, nothing outside. Fades at the far end."""
    best, tangent, along = None, None, 0.0
    run = 0.0
    for (a, b), L in zip(zip(CURRENT, CURRENT[1:]), SEG_LEN):
        t = (b - a) / L
        k = max(0.0, min(L, (p - a).dot(t)))
        d = (p - (a + t * k)).length()
        if best is None or d < best:
            best, tangent, along = d, t, run + k
        run += L
    if best > CURRENT_HALF_W:
        return V()
    edge = 1.0 - (best / CURRENT_HALF_W) ** 2
    end = min(1.0, (CURRENT_LEN - along) / 400)
    return tangent * CURRENT_SPEED * edge * max(0.0, end)


def dist_to_current(p):
    out = math.inf
    for (a, b), L in zip(zip(CURRENT, CURRENT[1:]), SEG_LEN):
        t = (b - a) / L
        k = max(0.0, min(L, (p - a).dot(t)))
        out = min(out, (p - (a + t * k)).length())
    return out


class Rock:
    __slots__ = ("pos", "r", "pts", "a0", "spin", "ice")

    def __init__(self, p, r, rng, ice):
        self.pos = V(p)
        self.r = r
        n = rng.randint(8, 12)
        self.pts = []
        for i in range(n):
            a = math.tau * (i + rng.uniform(-0.25, 0.25)) / n
            d = r * rng.uniform(0.8, 1.08)
            self.pts.append((math.cos(a) * d, math.sin(a) * d))
        self.a0 = rng.uniform(0, math.tau)
        self.spin = rng.uniform(-0.25, 0.25) * (40 / max(r, 20))
        self.ice = ice


class World:
    def __init__(self, seed=11):
        rng = random.Random(seed)
        self.rocks = []
        self.grid = {}

        def clear_of_lanes(p, r):
            if any((p - pos(s)).length() < 520 + r for s in STATION_IDS):
                return False
            if abs(p.x) < 320 + r and -200 < p.y < 4400:  # buoy line
                return False
            return dist_to_current(p) > CURRENT_HALF_W + 220 + r

        def add(p, r, ice):
            if all((p - o.pos).length() > r + o.r + 14 for o in self.near(p, r + 160)):
                rock = Rock(p, r, rng, ice)
                self.rocks.append(rock)
                self.grid.setdefault(self.cell(p), []).append(rock)

        for _ in range(60):  # Rimewater ice
            d = ICE_FIELD_R * math.sqrt(rng.random())
            a = rng.uniform(0, math.tau)
            p = ICE_FIELD + V(math.cos(a), math.sin(a)) * d
            if (p - pos("wharf")).length() > 480 and dist_to_current(p) > CURRENT_HALF_W + 60:
                add(p, rng.uniform(24, 58), True)
        for _ in range(500):  # plain rocks, sparse
            p = V(rng.uniform(-4000, 9000), rng.uniform(-4500, 6500))
            r = rng.choice((rng.uniform(16, 40), rng.uniform(30, 80)))
            if clear_of_lanes(p, r) and (p - ICE_FIELD).length() > ICE_FIELD_R + 300 and rng.random() < 0.45:
                add(p, r, False)

    @staticmethod
    def cell(p):
        return int(p[0] // CELL), int(p[1] // CELL)

    def in_rect(self, x0, y0, x1, y1):
        cx0, cy0 = self.cell((x0 - 160, y0 - 160))
        cx1, cy1 = self.cell((x1 + 160, y1 + 160))
        for cx in range(cx0, cx1 + 1):
            for cy in range(cy0, cy1 + 1):
                yield from self.grid.get((cx, cy), ())

    def near(self, p, radius):
        return self.in_rect(p[0] - radius, p[1] - radius, p[0] + radius, p[1] + radius)

    def ice_in_reach(self, p):
        """The nearest ice rock whose edge is within mining range of p, or None."""
        best, best_d = None, MINE_RANGE
        for rock in self.near(p, MINE_RANGE + 160):
            if rock.ice:
                d = (rock.pos - p).length() - rock.r
                if d < best_d:
                    best, best_d = rock, d
        return best

    def collide(self, ship, ship_r):
        """Push the ship out of rocks and drop the speed going into them. Returns hit speed."""
        hit = 0.0
        for rock in self.near(ship.pos, ship_r + 160):
            d = ship.pos - rock.pos
            min_d = rock.r * 0.92 + ship_r
            dist2 = d.length_squared()
            if 0.01 < dist2 < min_d * min_d:
                n = d / math.sqrt(dist2)
                ship.pos = rock.pos + n * min_d
                into = ship.vel.dot(n)
                if into < 0:
                    ship.vel -= n * into * 1.2  # a tiny bounce
                    ship.vel *= 0.85
                    hit = max(hit, -into)
        return hit
