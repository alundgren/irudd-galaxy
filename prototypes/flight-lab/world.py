"""The test field: asteroids, one station and a few far-off markers."""
import math
import random

from pygame.math import Vector2 as V

from palette import PURPLE, SKY, VERMILLION, YELLOW

STATION = V(0, 0)
DOCK_R = 80  # fly inside this to dock
RING_R = 260  # decorative ring, not solid
FIELD_R = 16000
CELL = 512

MARKERS = [
    ("North Beacon", V(0, -6000), YELLOW),
    ("East Relay", V(9000, 1500), PURPLE),
    ("Far Wreck", V(-11000, 8000), VERMILLION),
    ("Deep Buoy", V(14000, -12000), SKY),
]
ARRIVE_R = 300


class Rock:
    __slots__ = ("pos", "r", "pts", "a0", "spin", "ice")

    def __init__(self, pos, r, rng):
        self.pos = V(pos)
        self.r = r
        n = rng.randint(8, 13)
        self.pts = []
        for i in range(n):
            a = math.tau * (i + rng.uniform(-0.25, 0.25)) / n
            d = r * rng.uniform(0.78, 1.1)
            self.pts.append((math.cos(a) * d, math.sin(a) * d))
        self.a0 = rng.uniform(0, math.tau)
        self.spin = rng.uniform(-0.4, 0.4) * (40 / max(r, 20))
        self.ice = rng.random() < 0.25


class World:
    def __init__(self, seed=7):
        rng = random.Random(seed)
        self.rocks = []
        self.grid = {}
        keep_clear = [(STATION, RING_R + 600)] + [(p, 500) for _, p, _ in MARKERS]

        def free(p, r):
            return p.length() < FIELD_R and all((p - c).length() > cr + r for c, cr in keep_clear)

        def add(p, r):
            if free(p, r) and all((p - o.pos).length() > r + o.r + 6 for o in self.near(p, r + 160)):
                rock = Rock(p, r, rng)
                self.rocks.append(rock)
                self.grid.setdefault(self.cell(p), []).append(rock)

        for _ in range(80):  # clusters
            c = V(rng.uniform(-FIELD_R, FIELD_R), rng.uniform(-FIELD_R, FIELD_R))
            spread = rng.uniform(300, 1200)
            for _ in range(rng.randint(8, 32)):
                d = spread * math.sqrt(rng.random())
                a = rng.uniform(0, math.tau)
                r = rng.choice((rng.uniform(14, 40), rng.uniform(30, 75), rng.uniform(70, 140)))
                add(c + V(math.cos(a), math.sin(a)) * d, r)
        for _ in range(500):  # loose rocks
            add(V(rng.uniform(-FIELD_R, FIELD_R), rng.uniform(-FIELD_R, FIELD_R)), rng.uniform(12, 50))
        for i in range(14):  # a small ring of rocks near home so there is something to fly past
            a = math.tau * i / 14 + rng.uniform(-0.1, 0.1)
            add(V(math.cos(a), math.sin(a)) * rng.uniform(950, 1250), rng.uniform(25, 60))

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
