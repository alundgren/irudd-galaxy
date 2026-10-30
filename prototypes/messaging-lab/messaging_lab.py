# /// script
# requires-python = ">=3.10"
# dependencies = ["pygame-ce>=2.5.6"]
# ///
"""Messaging lab: fly, mine, dock and compare three ways NPCs talk to the player.

Run:   uv run messaging_lab.py        (or: python messaging_lab.py after pip install -r requirements.txt)
Check: uv run messaging_lab.py --selftest   (headless, prints PASS/FAIL)
"""
import math
import os
import random
import sys
import time

import pygame
from pygame.math import Vector2 as V

import world
from flight import SETTINGS, Controls, Ship, fly, steer
from palette import (AMBER, BLACK, GREEN, ICE_FILL, LAMP_DARK, PAD_FILL, PANEL, PURPLE, RING_DARK, ROCK_FILL, SKY,
                     SPACE, VERMILLION, WHITE)
from story import HOLD_CAP, LENS_PRICE, MODES, PEOPLE, STATION_NAMES, Story
from world import DOCK_R, ICE_FIELD, ICE_FIELD_R, World

HERE = os.path.dirname(os.path.abspath(__file__))
SHOT_DIR = os.path.join(HERE, "screenshots")
SIZE = (1280, 800)
SHIP_R = 17
# a plain regular face per platform; pygame's own font if none is there
FONT_FILES = [
    os.path.join(os.environ.get("WINDIR", ""), "Fonts", "segoeui.ttf"),
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
]

PERSON_COLOR = {"ilse": AMBER, "orrin": VERMILLION, "odo": GREEN, "tamsin": PURPLE, "rook": SKY}
STATION_COLOR = {"wharf": AMBER, "kettle": GREEN, "hook": VERMILLION}
STATION_R = {"wharf": 190, "kettle": 312, "hook": 220}  # how far the label sits below the centre

# ship outline, nose along +x (copied from the flight lab)
SS = 1.25
HULL = [(24, 0), (12, -9), (-4, -12), (-14, -11), (-18, -5), (-18, 5), (-14, 11), (-4, 12), (12, 9)]
POD_L = [(4, -10), (-10, -19), (-18, -18), (-15, -9)]
POD_R = [(x, -y) for x, y in POD_L]
STRIPE = [(-7, -11.4), (-2, -11.9), (-2, 11.9), (-7, 11.4)]
NOZZLE = [(-17, -5), (-23, -6.5), (-23, 6.5), (-17, 5)]
HULL, POD_L, POD_R, STRIPE, NOZZLE = ([(x * SS, y * SS) for x, y in p] for p in (HULL, POD_L, POD_R, STRIPE, NOZZLE))

# parallax, cell size, stars per cell, color, radius
STAR_LAYERS = [(0.2, 360, 3, SKY, 1), (0.5, 360, 2, PURPLE, 1), (1.0, 320, 2, WHITE, 1.5)]


def wrap(font, text, width):
    words, lines, cur = text.split(), [], ""
    for w in words:
        t = (cur + " " + w).strip()
        if not cur or font.size(t)[0] <= width:
            cur = t
        else:
            lines.append(cur)
            cur = w
    return lines + [cur] if cur else lines or [""]


def mix(a, b, k):
    k = max(0.0, min(1.0, k))
    return tuple(int(x + (y - x) * k) for x, y in zip(a, b))


def font(size, italic=False, bold=False):
    f = None
    for path in FONT_FILES:
        if os.path.isfile(path):
            try:
                f = pygame.font.Font(path, size)
                break
            except (OSError, pygame.error):
                pass
    if f is None:
        f = pygame.font.Font(None, int(size * 1.3))
    f.bold, f.italic = bold, italic
    return f


class App:
    def __init__(self, screen, mode=1):
        self.screen = screen
        self.W, self.H = screen.get_size()
        self.world = World()
        self.f_body = font(22)
        self.f_small = font(18)
        self.f_name = font(17, bold=True)
        self.f_title = font(28)
        self.f_big = font(30, bold=True)
        self.f_ital = font(22, italic=True)
        self.f_ital_title = font(30, italic=True)
        self.stars = {}
        self.glows = {}
        rng = random.Random(3)
        self.lanterns = [(rng.uniform(0, world.CURRENT_LEN), rng.uniform(-1, 1), rng.uniform(0.75, 1.0),
                          rng.uniform(0, math.tau)) for _ in range(260)]
        # hints the player has learned; these survive a mode switch
        self.paused_once = False
        self.undocked_once = False
        self.mined_once = False
        self.mouse = (0, 0)
        self.hits = []
        self.restart(mode)

    # ------------------------------------------------------------ state
    def restart(self, mode):
        self.story = Story(mode)
        self.ship = Ship(world.pos("wharf"))
        self.state, self.at = "docked", "wharf"
        self.undocked_from, self.armed = "wharf", False
        self.dock_t, self.dock_from = 0.0, V()
        self.cam = V(self.ship.pos)
        self.zoom = SETTINGS.zoom
        self.time = 0.0
        self.paused = False
        self.panel = None  # log | leads | chronicle | board
        self.panel_rect = pygame.Rect(0, 0, 0, 0)
        self.replay = None  # [visit, index] while replaying a log line
        self.letter = None  # letter open on the board
        self.log_scroll = 0
        self.seen_log = self.seen_leads = 0
        self.toast, self.toast_t = None, 0.0
        self.flash, self.flash_t = f"Mode {mode}: {MODES[mode]}. Story restarted.", 3.0
        self.ring_glow = self.lamp_glow = 0.0
        self.near_ice = self.mining_rock = None
        self.in_current = False
        self.lmb = False
        self.flame = 0.0
        self.story.dock("wharf")

    @property
    def mode(self):
        return self.story.mode

    def undock(self):
        self.story.undock()
        self.undocked_from, self.armed = self.at, False
        self.state, self.at = "free", None
        self.undocked_once = True
        if self.panel == "board":
            self.close_panel()

    def begin_dock(self, sid):
        self.state, self.at = "docking", sid
        self.dock_t, self.dock_from = 0.0, V(self.ship.pos)
        self.lmb = False

    def tow_home(self):
        if self.state == "docked":
            if self.at == "wharf":
                return
            self.undock()
        self.ship.pos = V(world.pos("wharf")) + V(0, 1)
        self.ship.hold_still()
        self.cam = V(self.ship.pos)
        self.begin_dock("wharf")
        self.flash, self.flash_t = "Towed to Lantern Wharf.", 2.5

    def close_panel(self):
        self.panel, self.replay, self.letter = None, None, None

    def open_panel(self, name):
        if self.panel == name:
            self.close_panel()
            return
        self.close_panel()
        self.panel = name
        self.log_scroll = 0
        if name == "log":
            self.seen_log = len(self.story.log)
        elif name == "leads":
            self.seen_leads = len(self.story.heard)
        elif name == "chronicle":
            self.story.chronicle_unread = 0

    def toggle_pause(self):
        self.paused = not self.paused
        self.paused_once = True
        self.lmb = False

    # ------------------------------------------------------------ simulation
    def step(self, dt, c):
        if self.paused:
            return
        self.time += dt
        ship, s, story = self.ship, SETTINGS, self.story
        thrust, speed, brake = steer(ship, s, c, dt)
        if self.state == "docked":
            ship.pos = V(world.pos(self.at))
            ship.hold_still()
            if thrust is not None:
                self.undock()
        elif self.state == "docking":
            self.dock_t = min(1.0, self.dock_t + dt / 0.4)
            k = self.dock_t * self.dock_t * (3 - 2 * self.dock_t)
            ship.pos = self.dock_from.lerp(world.pos(self.at), k)
            ship.hold_still()
            if self.dock_t >= 1:
                self.state = "docked"
                story.dock(self.at)
        self.near_ice = self.mining_rock = None
        self.in_current = False
        if self.state == "free":
            fly(ship, s, thrust, speed, brake, c.boost, dt)
            drift = world.current_drift(ship.pos)
            ship.pos += drift * dt
            self.in_current = drift.length_squared() > 1
            self.world.collide(ship, SHIP_R)
            self.check_docking()
            if (ship.pos - ICE_FIELD).length() < ICE_FIELD_R + 120:
                story.visit_place("rimewater")
            self.near_ice = self.world.ice_in_reach(ship.pos)
            if c.mine and self.near_ice and story.ice < HOLD_CAP:
                self.mining_rock = self.near_ice
                story.mine(dt)
                self.mined_once = True
        for ev in story.events:
            if ev == "hold_full":
                self.toast, self.toast_t = "Hold full", 4.0
        story.events.clear()
        self.ring_glow += max(-dt, min(dt * 0.8, story.ring_segments - self.ring_glow))
        self.lamp_glow += max(-dt, min(dt * 0.6, (1.0 if story.lamp_lit else 0.0) - self.lamp_glow))
        self.flame += ((1.0 if ship.thrusting else 0.0) - self.flame) * min(1.0, dt * 12)
        self.cam += (ship.pos - self.cam) * (1 - math.exp(-6 * dt))
        self.toast_t -= dt
        self.flash_t -= dt

    def check_docking(self):
        p = self.ship.pos
        if not self.armed and (p - world.pos(self.undocked_from)).length() > DOCK_R + 120:
            self.armed = True
        for sid in world.STATION_IDS:
            if (p - world.pos(sid)).length() < DOCK_R and (sid != self.undocked_from or self.armed):
                self.begin_dock(sid)
                return

    # ------------------------------------------------------------ input
    def controls(self):
        k = pygame.key.get_pressed()

        def held(*keys):
            return any(k[x] for x in keys)

        return Controls(
            up=held(pygame.K_w, pygame.K_UP), down=held(pygame.K_s, pygame.K_DOWN),
            left=held(pygame.K_a, pygame.K_LEFT), right=held(pygame.K_d, pygame.K_RIGHT),
            boost=held(pygame.K_LSHIFT, pygame.K_RSHIFT), brake=held(pygame.K_SPACE),
            mine=held(pygame.K_m) or self.lmb)

    def talk_click(self):
        ln = self.story.line
        if self.state == "docked" and ln and ln.kind != "choice":
            self.story.advance()

    def handle(self, e):
        """One event. Returns "quit" to leave."""
        if e.type == pygame.QUIT:
            return "quit"
        if e.type in (pygame.VIDEORESIZE, pygame.WINDOWSIZECHANGED):
            self.screen = pygame.display.get_surface()
            self.W, self.H = self.screen.get_size()
        elif e.type == pygame.MOUSEMOTION:
            self.mouse = e.pos
        elif e.type == pygame.KEYDOWN:
            k = e.key
            if k in (pygame.K_F1, pygame.K_F2, pygame.K_F3):
                self.restart({pygame.K_F1: 1, pygame.K_F2: 2, pygame.K_F3: 3}[k])
            elif k == pygame.K_ESCAPE and self.panel and not self.paused:
                self.close_panel()
            elif k in (pygame.K_p, pygame.K_ESCAPE):
                self.toggle_pause()
            elif self.paused:
                if k == pygame.K_q:
                    return "quit"
            elif k == pygame.K_l and self.mode in (1, 2):
                self.open_panel("log" if self.mode == 1 else "leads")
            elif k == pygame.K_c and self.mode == 3:
                self.open_panel("chronicle")
            elif k == pygame.K_b and self.mode == 3 and self.state == "docked" and self.story.board(self.at):
                self.open_panel("board")
            elif k in (pygame.K_RETURN, pygame.K_KP_ENTER):
                if self.panel == "log" and self.replay:
                    self.replay_next()
                elif not self.panel:
                    self.talk_click()
            elif k == pygame.K_r:
                self.tow_home()
            elif k == pygame.K_F12:
                self.screenshot()
        elif e.type == pygame.MOUSEBUTTONDOWN and e.button == 1:
            self.mouse = e.pos
            for rect, fn, _tag in reversed(self.hits):
                if rect.collidepoint(e.pos):
                    fn()
                    return None
            if self.paused:
                return None
            if self.panel:
                if not self.panel_rect.collidepoint(e.pos):
                    self.close_panel()
                return None
            if self.state == "docked":
                self.talk_click()
            elif self.state == "free":
                self.lmb = True
        elif e.type == pygame.MOUSEBUTTONUP and e.button == 1:
            self.lmb = False
        elif e.type == pygame.MOUSEWHEEL and self.panel == "log" and not self.replay:
            self.log_scroll = max(0, self.log_scroll - e.y)
        return None

    def run(self):
        clock = pygame.time.Clock()
        pygame.key.stop_text_input()
        while True:
            dt = min(clock.tick(120) / 1000.0, 1 / 30)
            for e in pygame.event.get():
                if self.handle(e) == "quit":
                    return 0
            if pygame.mouse.get_focused():
                self.mouse = pygame.mouse.get_pos()
            self.step(dt, self.controls())
            self.draw()
            pygame.display.flip()

    def screenshot(self):
        os.makedirs(SHOT_DIR, exist_ok=True)
        name = time.strftime("messaging-lab-%Y%m%d-%H%M%S.png")
        pygame.image.save(self.screen, os.path.join(SHOT_DIR, name))
        self.flash, self.flash_t = f"Saved screenshots/{name}", 2.5

    # ------------------------------------------------------------ view helpers
    def w2s(self, p):
        return ((p[0] - self.cam.x) * self.zoom + self.W / 2, (p[1] - self.cam.y) * self.zoom + self.H / 2)

    def on_screen(self, p, margin):
        x, y = self.w2s(p)
        m = margin * self.zoom
        return -m < x < self.W + m and -m < y < self.H + m

    def xform(self, pts, pos, ang):
        c, s = math.cos(ang), math.sin(ang)
        z, ox, oy = self.zoom, self.W / 2 - self.cam.x * self.zoom, self.H / 2 - self.cam.y * self.zoom
        return [((pos[0] + x * c - y * s) * z + ox, (pos[1] + x * s + y * c) * z + oy) for x, y in pts]

    def poly(self, pts, pos, ang, color, w=2):
        sp = self.xform(pts, pos, ang)
        pygame.draw.polygon(self.screen, color, sp)
        if w:
            pygame.draw.polygon(self.screen, BLACK, sp, w)

    def text(self, f, s, pos, color=WHITE, anchor="topleft"):
        img = f.render(s, True, color)
        r = img.get_rect(**{anchor: (int(pos[0]), int(pos[1]))})
        self.screen.blit(img, r)
        return r

    def glow(self, sp, radius, color, alpha):
        r = max(2, int(radius))
        key = (r, color, int(alpha))
        surf = self.glows.get(key)
        if surf is None:
            if len(self.glows) > 300:
                self.glows.clear()
            surf = pygame.Surface((2 * r, 2 * r), pygame.SRCALPHA)
            for i in range(5):
                pygame.draw.circle(surf, (*color, int(alpha * (i + 1) / 5)), (r, r), r * (1 - i / 5))
            self.glows[key] = surf
        self.screen.blit(surf, (sp[0] - r, sp[1] - r))

    def arc_poly(self, c, r0, r1, a0, a1, steps=20):
        outer = [self.w2s((c.x + math.cos(a0 + (a1 - a0) * i / steps) * r1,
                           c.y + math.sin(a0 + (a1 - a0) * i / steps) * r1)) for i in range(steps + 1)]
        inner = [self.w2s((c.x + math.cos(a0 + (a1 - a0) * i / steps) * r0,
                           c.y + math.sin(a0 + (a1 - a0) * i / steps) * r0)) for i in range(steps, -1, -1)]
        return outer + inner

    def hit(self, rect, fn, tag):
        self.hits.append((pygame.Rect(rect), fn, tag))

    def button(self, rect, label, fn, tag, enabled=True, light=True, f=None):
        f = f or self.f_body
        rect = pygame.Rect(rect)
        hover = enabled and rect.collidepoint(self.mouse)
        if light:
            fill, edge, ink = (SKY if hover else WHITE), (BLACK if enabled else PURPLE), BLACK
        else:
            fill, edge, ink = PANEL, (SKY if hover else WHITE), WHITE
        pygame.draw.rect(self.screen, fill, rect, border_radius=8)
        pygame.draw.rect(self.screen, edge, rect, 2 if (hover or light) else 1, border_radius=8)
        self.text(f, label, rect.center, ink, "center")
        if enabled:
            self.hit(rect, fn, tag)
        return rect

    def button_size(self, label, f=None):
        w, h = (f or self.f_body).size(label)
        return w + 28, h + 14

    def dot(self, pos):
        pygame.draw.circle(self.screen, AMBER, pos, 6)
        pygame.draw.circle(self.screen, BLACK, pos, 6, 1)

    # ------------------------------------------------------------ drawing: world
    def draw(self):
        self.hits = []
        self.screen.fill(SPACE)
        self.draw_stars()
        self.draw_current()
        self.draw_buoys()
        self.draw_rocks()
        for sid in world.STATION_IDS:
            self.draw_station(sid)
        self.draw_beam()
        self.draw_ship()
        self.draw_labels()
        if self.state == "docked" and not self.panel:
            self.draw_talk()
        self.draw_hud()
        if self.panel:
            self.draw_panel()
        if self.paused:
            self.draw_pause()

    def draw_stars(self):
        for li, (par, cell, count, color, rad) in enumerate(STAR_LAYERS):
            z = 1 + (self.zoom - 1) * par
            ox, oy = self.cam.x * par, self.cam.y * par
            hw, hh = self.W / 2 / z, self.H / 2 / z
            for cx in range(int((ox - hw) // cell), int((ox + hw) // cell) + 1):
                for cy in range(int((oy - hh) // cell), int((oy + hh) // cell) + 1):
                    key = (li, cx, cy)
                    pts = self.stars.get(key)
                    if pts is None:
                        if len(self.stars) > 20000:
                            self.stars.clear()
                        rng = random.Random(hash(key))
                        pts = [((cx + rng.random()) * cell, (cy + rng.random()) * cell) for _ in range(count)]
                        self.stars[key] = pts
                    for x, y in pts:
                        pygame.draw.circle(self.screen, color, ((x - ox) * z + self.W / 2, (y - oy) * z + self.H / 2),
                                           max(1.0, rad * z if par == 1.0 else rad))

    def draw_current(self):
        z = self.zoom
        for s0, side, speed, phase in self.lanterns:
            s = (s0 + world.CURRENT_SPEED * speed * self.time) % world.CURRENT_LEN
            p, t = world.current_at(s)
            n = V(-t.y, t.x)
            wob = math.sin(self.time * 0.7 + phase) * 0.15
            wp = p + n * (side * abs(side) + wob) * world.CURRENT_HALF_W * 0.8
            if not self.on_screen(wp, 40):
                continue
            sp = self.w2s(wp)
            fade = min(1.0, s / 300, (world.CURRENT_LEN - s) / 300)
            self.glow(sp, 16 * z, AMBER, 70 * fade)
            pygame.draw.line(self.screen, AMBER, self.w2s(wp - t * 26 * speed), sp, max(1, int(2 * z)))
            pygame.draw.circle(self.screen, AMBER, sp, max(1.5, 3.4 * z * (0.5 + 0.5 * fade)))

    def draw_buoys(self):
        z = self.zoom
        for b in world.BUOYS:
            if not self.on_screen(b, 40):
                continue
            sp = self.w2s(b)
            self.glow(sp, 34 * z, SKY, 60)
            pygame.draw.circle(self.screen, SKY, sp, 9 * z)
            pygame.draw.circle(self.screen, WHITE, sp, 9 * z, max(1, int(2 * z)))

    def draw_rocks(self):
        z = self.zoom
        x0, y0 = self.cam.x - self.W / 2 / z, self.cam.y - self.H / 2 / z
        x1, y1 = self.cam.x + self.W / 2 / z, self.cam.y + self.H / 2 / z
        for rock in self.world.in_rect(x0, y0, x1, y1):
            if not self.on_screen(rock.pos, rock.r + 10):
                continue
            fill, edge = (ICE_FILL, SKY) if rock.ice else (ROCK_FILL, PURPLE)
            pts = self.xform(rock.pts, rock.pos, rock.a0 + rock.spin * self.time)
            pygame.draw.polygon(self.screen, fill, pts)
            pygame.draw.polygon(self.screen, edge, pts, 2)

    def draw_pad(self, c):
        z = self.zoom
        pygame.draw.circle(self.screen, PAD_FILL, self.w2s(c), DOCK_R * z)
        for i in range(12):  # dashed dock ring
            a0 = -self.time * 0.5 + i * math.tau / 12
            pts = [self.w2s((c.x + math.cos(a0 + t * 0.3) * DOCK_R, c.y + math.sin(a0 + t * 0.3) * DOCK_R))
                   for t in range(4)]
            pygame.draw.lines(self.screen, SKY, False, pts, max(2, int(4 * z)))

    def draw_station(self, sid):
        c = world.pos(sid)
        if not self.on_screen(c, 420):
            return
        z = self.zoom
        lw = max(2, int(6 * z))
        if sid == "wharf":
            pygame.draw.circle(self.screen, WHITE, self.w2s(c), 150 * z, max(1, int(3 * z)))
            for side in (-1, 1):
                pygame.draw.line(self.screen, WHITE, self.w2s(c + V(150 * side, 0)), self.w2s(c + V(330 * side, 0)), lw)
                for x in (200, 265, 330):
                    post = c + V(x * side, 0)
                    pygame.draw.line(self.screen, WHITE, self.w2s(post), self.w2s(post + V(0, -34)), max(1, int(3 * z)))
                    lamp = self.w2s(post + V(0, -42))
                    self.glow(lamp, 30 * z, AMBER, 90)
                    pygame.draw.circle(self.screen, AMBER, lamp, 9 * z)
                    pygame.draw.circle(self.screen, BLACK, lamp, 9 * z, 1)
        elif sid == "kettle":
            for i in range(4):
                a = math.pi / 4 + i * math.tau / 4
                pygame.draw.line(self.screen, WHITE, self.w2s(c + V(math.cos(a), math.sin(a)) * 90),
                                 self.w2s(c + V(math.cos(a), math.sin(a)) * 300), lw)
            for i in range(4):  # the greenhouse ring, four panels that light one by one
                lit = max(0.0, min(1.0, self.ring_glow - i))
                a = i * math.tau / 4
                a0, a1 = a - math.radians(38), a + math.radians(38)
                if lit > 0:
                    mid = c + V(math.cos(a), math.sin(a)) * 280
                    self.glow(self.w2s(mid), 170 * z, GREEN, 70 * lit)
                pts = self.arc_poly(c, 262, 298, a0, a1)
                pygame.draw.polygon(self.screen, mix(RING_DARK, GREEN, lit), pts)
                pygame.draw.polygon(self.screen, GREEN if lit < 0.5 else WHITE, pts, max(1, int(2 * z)))
                if lit > 0.3:
                    for j in range(5):
                        w = a0 + (a1 - a0) * (j + 0.5) / 5
                        pygame.draw.circle(self.screen, AMBER, self.w2s(c + V(math.cos(w), math.sin(w)) * 280),
                                           max(2, 5 * z * lit))
        elif sid == "hook":
            pts = self.arc_poly(c, 178, 204, math.radians(-150), math.radians(150), 32)
            pygame.draw.polygon(self.screen, VERMILLION, pts)
            pygame.draw.polygon(self.screen, BLACK, pts, max(1, int(2 * z)))
            pygame.draw.line(self.screen, WHITE, self.w2s(c + V(0, -204)), self.w2s(c + V(0, -270)), lw)
            lamp = self.w2s(c + V(0, -288))
            if self.lamp_glow > 0:
                self.glow(lamp, 110 * z, AMBER, 110 * self.lamp_glow)
            pygame.draw.circle(self.screen, mix(LAMP_DARK, AMBER, self.lamp_glow), lamp, 18 * z)
            pygame.draw.circle(self.screen, AMBER, lamp, 18 * z, max(1, int(2 * z)))
        self.draw_pad(c)

    def draw_labels(self):
        z = self.zoom
        for sid in world.STATION_IDS:
            c = world.pos(sid)
            if self.on_screen(c, 420):
                sx, sy = self.w2s(c)
                self.text(self.f_title, world.name(sid), (sx, sy + STATION_R[sid] * z), WHITE, "midtop")
        if self.mode == 3 and self.state == "docked" and self.story.board(self.at):
            sx, sy = self.w2s(world.pos(self.at))
            n, new = len(self.story.board(self.at)), self.story.unread(self.at)
            label = f"Board: {n} letter{'s' if n > 1 else ''}" + (f", {new} new" if new else "")
            w, h = self.button_size(label)
            r = pygame.Rect(0, 0, w, h)
            r.midtop = (sx, sy + STATION_R[self.at] * z + 38)
            self.button(r, label, lambda: self.open_panel("board"), "board", light=False)
            if new:
                self.dot((r.right - 4, r.top + 4))

    def draw_beam(self):
        rock = self.mining_rock
        if rock is None:
            return
        ship = self.ship
        nose = ship.pos + ship.fwd * 26
        to = rock.pos - nose
        end = rock.pos - to.normalize() * rock.r * 0.85 if to.length() > rock.r else rock.pos
        a, b = self.w2s(nose), self.w2s(end)
        pygame.draw.line(self.screen, SKY, a, b, 5)
        pygame.draw.line(self.screen, WHITE, a, b, 1)
        for i in range(3):
            k = (self.time * 2.3 + i / 3) % 1.0
            p = end.lerp(nose, k)
            pygame.draw.circle(self.screen, WHITE, self.w2s(p), 2.5)
        self.glow(b, 26 * self.zoom, SKY, 120)

    def draw_ship(self):
        ship = self.ship
        pos, ang = ship.pos, ship.angle
        if self.flame > 0.05:
            boosting = pygame.key.get_mods() & pygame.KMOD_SHIFT
            L = (8 + 16 * self.flame * (1.6 if boosting else 1.0) + 4 * abs(math.sin(self.time * 37))) * SS
            if ship.retro:
                nose = 26 * SS
                self.poly([(nose, -3.5 * SS), (nose + L * 0.55, 0), (nose, 3.5 * SS)], pos, ang, VERMILLION)
                self.poly([(nose, -1.8 * SS), (nose + L * 0.3, 0), (nose, 1.8 * SS)], pos, ang, AMBER, w=0)
            else:
                back = -23 * SS
                self.poly([(back, -5 * SS), (back - L, 0), (back, 5 * SS)], pos, ang, VERMILLION)
                self.poly([(back, -2.5 * SS), (back - L * 0.55, 0), (back, 2.5 * SS)], pos, ang, AMBER, w=0)
        self.poly(POD_L, pos, ang, AMBER)
        self.poly(POD_R, pos, ang, AMBER)
        self.poly(NOZZLE, pos, ang, SKY)
        self.poly(HULL, pos, ang, WHITE)
        self.poly(STRIPE, pos, ang, VERMILLION)
        cp = self.w2s(pos + ship.fwd * (9 * SS))
        pygame.draw.circle(self.screen, SKY, cp, 6 * SS * self.zoom)
        pygame.draw.circle(self.screen, BLACK, cp, 6 * SS * self.zoom, max(1, round(2 * self.zoom)))

    # ------------------------------------------------------------ drawing: talk
    def talk_anchor(self):
        sx, sy = self.w2s(world.pos(self.at))
        return sx, sy - 70 * self.zoom

    def draw_talk(self):
        ln = self.story.line
        if ln is None:
            return
        self.draw_bubble(ln, self.talk_anchor(), interactive=True)

    def draw_bubble(self, ln, tip, interactive, bounds=None):
        """A speech bubble (white, speaker tag, tail) or a receipt (dark ticket, no speaker) above tip."""
        bounds = bounds or self.screen.get_rect().inflate(-20, -20)
        pad, max_w = 16, 430
        if ln.kind == "receipt":
            lines = wrap(self.f_body, ln.text, max_w)
            lh = self.f_body.get_linesize()
            w = max(self.f_body.size(s)[0] for s in lines) + 2 * pad + 22
            r = pygame.Rect(0, 0, w, lh * len(lines) + 2 * pad)
            r.midbottom = (tip[0], tip[1] - 24)
            r.clamp_ip(bounds)
            pygame.draw.rect(self.screen, BLACK, r, border_radius=6)
            pygame.draw.rect(self.screen, AMBER, r, 2, border_radius=6)
            for i, s in enumerate(lines):
                self.text(self.f_body, s, (r.x + pad, r.y + pad + i * lh), WHITE)
            if interactive:
                self.next_mark(r, WHITE)
                self.hit(r, self.talk_click, "bubble")
            return r
        lines = wrap(self.f_body, ln.text, max_w)
        lh = self.f_body.get_linesize()
        opts = ln.options if (ln.kind == "choice" and interactive) else []
        sizes = [self.button_size(o[0], self.f_small) for o in opts]
        text_w = max(self.f_body.size(s)[0] for s in lines)
        rows, row = [], []
        for sz in sizes:  # lay buttons out in rows that fit the bubble
            if row and sum(x[0] for x in row) + 10 * len(row) + sz[0] > max_w:
                rows.append(row)
                row = []
            row.append(sz)
        if row:
            rows.append(row)
        btn_w = max((sum(x[0] for x in rw) + 10 * (len(rw) - 1) for rw in rows), default=0)
        w = max(text_w, btn_w, 120) + 2 * pad + (18 if ln.kind == "say" else 0)
        h = lh * len(lines) + 2 * pad + 6 + sum(x[0][1] + 10 for x in rows)
        r = pygame.Rect(0, 0, w, h)
        r.midbottom = (tip[0], tip[1] - 26)
        r.clamp_ip(bounds)
        tx = max(r.left + 24, min(r.right - 24, tip[0]))
        pygame.draw.polygon(self.screen, WHITE, [(tx - 14, r.bottom - 2), (tx + 14, r.bottom - 2), (tip[0], tip[1])])
        pygame.draw.rect(self.screen, WHITE, r, border_radius=12)
        color = PERSON_COLOR.get(ln.who, WHITE)
        name = PEOPLE.get(ln.who, "")
        nw, nh = self.f_name.size(name)
        tag = pygame.Rect(r.x + 12, r.y - nh // 2 - 6, nw + 16, nh + 6)
        pygame.draw.rect(self.screen, color, tag, border_radius=6)
        pygame.draw.rect(self.screen, BLACK, tag, 1, border_radius=6)
        self.text(self.f_name, name, tag.center, BLACK, "center")
        y = r.y + pad + 6
        for s in lines:
            self.text(self.f_body, s, (r.x + pad, y), BLACK)
            y += lh
        y += 6
        i = 0
        for rw in rows:
            x = r.x + pad
            for sz in rw:
                label, action, enabled = opts[i]
                self.button((x, y, sz[0], sz[1]), label, lambda a=action: self.story.choose(a), "opt:" + action,
                            enabled=enabled, f=self.f_small)
                x += sz[0] + 10
                i += 1
            y += rw[0][1] + 10
        if interactive and ln.kind == "say":
            self.next_mark(r, BLACK)
            self.hit(r, self.talk_click, "bubble")
        return r

    def next_mark(self, r, color):
        x, y = r.right - 18, r.bottom - 16
        bob = 2 * math.sin(self.time * 2.0)
        pygame.draw.polygon(self.screen, color, [(x - 6, y - 5 + bob), (x + 6, y - 5 + bob), (x, y + 3 + bob)])

    # ------------------------------------------------------------ drawing: HUD
    def hints(self):
        out = []
        if self.state == "docked" and not self.undocked_once:
            out.append("W: fly off to undock")
        if self.near_ice and not self.mined_once and self.story.ice < HOLD_CAP:
            out.append("Hold M or left mouse: mine")
        if not self.paused_once:
            out.append("P or Esc: pause")
        return out

    def draw_hud(self):
        story = self.story
        box = pygame.Rect(12, 12, 250, 84)
        pygame.draw.rect(self.screen, PANEL, box, border_radius=8)
        self.text(self.f_big, f"{story.credits} cr", (box.x + 14, box.y + 8), WHITE)
        if story.lens:
            t = pygame.Rect(0, 0, 60, 24)
            t.topright = (box.right - 12, box.y + 14)
            pygame.draw.rect(self.screen, GREEN, t, border_radius=5)
            self.text(self.f_name, "Lens", t.center, BLACK, "center")
        self.text(self.f_small, "Ice", (box.x + 14, box.y + 54), WHITE)
        bar = pygame.Rect(box.x + 52, box.y + 57, 120, 14)
        full = story.ice >= HOLD_CAP
        pygame.draw.rect(self.screen, BLACK, bar)
        pygame.draw.rect(self.screen, AMBER if full else SKY, (bar.x, bar.y, int(bar.w * story.ice / HOLD_CAP), bar.h))
        pygame.draw.rect(self.screen, WHITE, bar, 1)
        self.text(self.f_small, f"{story.ice}/{HOLD_CAP}", (bar.right + 10, box.y + 54), WHITE)
        if self.toast and self.toast_t > 0:
            w, h = self.f_body.size(self.toast)
            t = pygame.Rect(box.right + 10, bar.centery - h // 2 - 5, w + 20, h + 10)
            pygame.draw.rect(self.screen, AMBER, t, border_radius=6)
            self.text(self.f_body, self.toast, t.center, BLACK, "center")

        # the mode's own corner icon
        icon = {1: ("Log", "log"), 2: ("Leads", "leads"), 3: ("Chronicle", "chronicle")}[self.mode]
        new = {1: len(story.log) > self.seen_log, 2: len(story.heard) > self.seen_leads,
               3: story.chronicle_unread > 0}[self.mode]
        w, h = self.button_size(icon[0])
        r = pygame.Rect(0, 0, w + 26, h + 4)
        r.topright = (self.W - 12, 12)
        self.button(r, "", lambda: self.open_panel(icon[1]), "icon:" + icon[1], light=False)
        self.draw_icon(icon[1], (r.x + 20, r.centery))
        self.text(self.f_body, icon[0], (r.x + 36, r.centery), WHITE, "midleft")
        if new:
            self.dot((r.right - 4, r.top + 4))

        label = [f"Mode {self.mode}: {MODES[self.mode]}", "F1 F2 F3: switch mode (restarts story)"]
        lw = max(self.f_small.size(t)[0] for t in label) + 20
        back = pygame.Rect(self.W - 12 - lw, r.bottom + 8, lw, 50)
        pygame.draw.rect(self.screen, PANEL, back, border_radius=8)
        for i, t in enumerate(label):
            self.text(self.f_small, t, (back.right - 10, back.y + 4 + 22 * i), WHITE, "topright")
        hints = self.hints()
        if hints:
            s = "     ".join(hints)
            w, h = self.f_body.size(s)
            r = pygame.Rect(0, 0, w + 28, h + 12)
            r.midbottom = (self.W // 2, self.H - 12)
            pygame.draw.rect(self.screen, PANEL, r, border_radius=8)
            pygame.draw.rect(self.screen, WHITE, r, 1, border_radius=8)
            self.text(self.f_body, s, r.center, WHITE, "center")
        if self.flash_t > 0:
            w, h = self.f_body.size(self.flash)
            r = pygame.Rect(0, 0, w + 28, h + 12)
            r.midtop = (self.W // 2, 14)
            pygame.draw.rect(self.screen, PANEL, r, border_radius=8)
            pygame.draw.rect(self.screen, SKY, r, 1, border_radius=8)
            self.text(self.f_body, self.flash, r.center, WHITE, "center")

    def draw_icon(self, kind, c):
        x, y = c
        if kind == "log":  # a little book
            pygame.draw.rect(self.screen, SKY, (x - 9, y - 8, 8, 16))
            pygame.draw.rect(self.screen, SKY, (x + 1, y - 8, 8, 16))
            pygame.draw.line(self.screen, WHITE, (x, y - 8), (x, y + 8), 1)
        elif kind == "leads":  # a little card
            pygame.draw.rect(self.screen, GREEN, (x - 9, y - 7, 18, 14), border_radius=2)
            pygame.draw.line(self.screen, BLACK, (x - 5, y - 2), (x + 5, y - 2), 2)
            pygame.draw.line(self.screen, BLACK, (x - 5, y + 2), (x + 2, y + 2), 2)
        else:  # a diamond, the chronicle's mark
            pygame.draw.polygon(self.screen, AMBER, [(x, y - 9), (x + 7, y), (x, y + 9), (x - 7, y)])

    # ------------------------------------------------------------ drawing: panels
    def panel_frame(self, title, width=640, height=None, chronicle=False):
        h = min(self.H - 150, height or self.H - 150)
        r = pygame.Rect(0, 0, min(width, self.W - 60), h)
        r.center = (self.W // 2, self.H // 2 + 10)
        self.panel_rect = r
        pygame.draw.rect(self.screen, SPACE if chronicle else PANEL, r, border_radius=10)
        if chronicle:
            for y in (r.top + 4, r.bottom - 4):
                pygame.draw.line(self.screen, AMBER, (r.left + 30, y), (r.right - 30, y), 2)
            self.text(self.f_ital_title, title, (r.centerx, r.y + 22), WHITE, "midtop")
        else:
            pygame.draw.rect(self.screen, WHITE, r, 1, border_radius=10)
            self.text(self.f_title, title, (r.x + 20, r.y + 16), WHITE)
        w, h2 = self.button_size("Close", self.f_small)
        self.button((r.right - w - 14, r.y + (22 if chronicle else 14), w, h2), "Close", self.close_panel, "close",
                    light=False, f=self.f_small)
        return r

    def draw_panel(self):
        {"log": self.draw_log, "leads": self.draw_leads, "chronicle": self.draw_chronicle,
         "board": self.draw_board}[self.panel]()

    def draw_log(self):
        story = self.story
        if self.replay:
            return self.draw_replay()
        lh = self.f_body.get_linesize()
        pins = story.pinned()
        tall = 110 + sum(lh * len(wrap(self.f_body, t, 470)) + 20 for _, t in pins) + (24 if pins else 0)
        tall += 40 + sum(lh * len(wrap(self.f_body, v.text(), 536)) + 22 for v in story.log)
        r = self.panel_frame("Ship log", height=tall)
        x, y, w = r.x + 20, r.y + 64, r.w - 40
        if pins:
            for tag, s in pins:
                lines = wrap(self.f_body, s, w - 90)
                h = len(lines) * self.f_body.get_linesize() + 12
                pygame.draw.rect(self.screen, AMBER, (x, y, 5, h))
                self.text(self.f_name, tag.upper(), (x + 14, y + 8), AMBER)
                for i, s2 in enumerate(lines):
                    self.text(self.f_body, s2, (x + 80, y + 6 + i * self.f_body.get_linesize()), WHITE)
                y += h + 8
            y += 8
            pygame.draw.line(self.screen, WHITE, (x, y), (x + w, y), 1)
            y += 14
        if not story.log:
            self.text(self.f_body, "No visits yet. Undock to log this one.", (x, y), WHITE)
            return
        self.text(self.f_small, "Visits, newest first. Click one to replay it.", (x, y), WHITE)
        y += 30
        visits = list(reversed(story.log))
        self.log_scroll = min(self.log_scroll, max(0, len(visits) - 1))
        for vi, visit in enumerate(visits[self.log_scroll:]):
            lines = wrap(self.f_body, visit.text(), w - 24)
            h = len(lines) * self.f_body.get_linesize() + 14
            if y + h > r.bottom - 14:
                self.text(self.f_small, "Scroll for older visits", (x, r.bottom - 26), WHITE)
                break
            row = pygame.Rect(x, y, w, h)
            hover = row.collidepoint(self.mouse)
            pygame.draw.rect(self.screen, SKY if hover else WHITE, row, 2 if hover else 1, border_radius=6)
            for i, s in enumerate(lines):
                self.text(self.f_body, s, (x + 12, y + 7 + i * self.f_body.get_linesize()), WHITE)
            self.hit(row, lambda v=visit: self.start_replay(v), f"log_row:{vi + self.log_scroll}")
            y += h + 8

    def start_replay(self, visit):
        self.replay = [visit, 0]

    def replay_next(self):
        self.replay[1] += 1
        if self.replay[1] >= len(self.replay[0].lines):
            self.replay = None

    def draw_replay(self):
        visit, i = self.replay
        r = self.panel_frame(f"Replay: {STATION_NAMES[visit.station]}", height=440)
        body = pygame.Rect(r.x, r.y + 60, r.w, r.h - 60)
        self.hit(body, self.replay_next, "replay")
        w, h = self.button_size("Back to log", self.f_small)
        self.button((r.x + 20, r.bottom - h - 14, w, h), "Back to log", lambda: setattr(self, "replay", None), "back",
                    light=False, f=self.f_small)
        c = (r.centerx, r.bottom - 120)
        col = STATION_COLOR[visit.station]
        pygame.draw.circle(self.screen, PAD_FILL, c, 40)
        pygame.draw.circle(self.screen, col, c, 58, 6)
        self.text(self.f_body, STATION_NAMES[visit.station], (c[0], c[1] + 66), WHITE, "midtop")
        self.draw_bubble(visit.lines[i], (c[0], c[1] - 50), interactive=False, bounds=r.inflate(-30, -80))
        self.text(self.f_small, f"{i + 1} of {len(visit.lines)}. Click to continue.", (r.right - 20, r.bottom - 22),
                  WHITE, "bottomright")

    def draw_leads(self):
        lh = self.f_body.get_linesize()
        cards = [(name, wrap(self.f_body, how + (f" ({who})" if who else ""), 520), been)
                 for name, how, who, been in self.story.leads()]
        r = self.panel_frame("Leads", width=600, height=96 + sum(62 + lh * len(c[1]) for c in cards))
        x, y, w = r.x + 24, r.y + 70, r.w - 48
        for name, lines, been in cards:
            card = pygame.Rect(x, y, w, 50 + lh * len(lines))
            pygame.draw.rect(self.screen, WHITE, card, 1, border_radius=8)
            self.text(self.f_title, name, (x + 16, y + 10), WHITE)
            label = "been there" if been else "heard of"
            tw, th = self.f_small.size(label)
            tag = pygame.Rect(0, 0, tw + 18, th + 8)
            tag.topright = (card.right - 14, y + 14)
            if been:
                pygame.draw.rect(self.screen, GREEN, tag, border_radius=10)
                self.text(self.f_small, label, tag.center, BLACK, "center")
            else:
                pygame.draw.rect(self.screen, SKY, tag, 2, border_radius=10)
                self.text(self.f_small, label, tag.center, WHITE, "center")
            for i, s in enumerate(lines):
                self.text(self.f_body, s, (x + 16, y + 46 + i * lh), WHITE)
            y += card.h + 12

    def draw_chronicle(self):
        lines = self.story.chronicle
        r = self.panel_frame("Chronicle", width=620, height=150 + 70 * max(1, len(lines)), chronicle=True)
        y = r.y + 84
        if not lines:
            self.text(self.f_ital, "Nothing has changed yet.", (r.centerx, y), WHITE, "midtop")
        for i, s in enumerate(lines):
            if i:
                cx, cy = r.centerx, y - 22
                pygame.draw.polygon(self.screen, AMBER, [(cx, cy - 6), (cx + 5, cy), (cx, cy + 6), (cx - 5, cy)])
            self.text(self.f_ital, s, (r.centerx, y), WHITE, "midtop")
            y += 70

    def draw_board(self):
        sid = self.at
        if self.state != "docked" or not sid:
            self.close_panel()
            return
        letters = self.story.board(sid)
        if self.letter is not None:
            return self.draw_letter(sid, self.letter)
        r = self.panel_frame(f"{STATION_NAMES[sid]} board", width=600, height=120 + 76 * len(letters))
        x, y, w = r.x + 24, r.y + 70, r.w - 48
        for li, lt in enumerate(letters):
            row = pygame.Rect(x, y, w, 64)
            hover = row.collidepoint(self.mouse)
            pygame.draw.rect(self.screen, WHITE, row, border_radius=6)
            pygame.draw.rect(self.screen, SKY if hover else BLACK, row, 3 if hover else 1, border_radius=6)
            self.text(self.f_body, lt.title, (x + 16, y + 8), BLACK)
            self.text(self.f_small, "From " + lt.sender, (x + 16, y + 36), BLACK)
            if not lt.read:
                self.dot((row.right - 18, row.centery))
            self.hit(row, lambda lt=lt: self.open_letter(lt), f"letter:{li}")
            y += 76

    def open_letter(self, lt):
        lt.read = True
        self.letter = lt

    def draw_letter(self, sid, lt):
        story = self.story
        body = lt.body[:-1]
        r = self.panel_frame(f"{STATION_NAMES[sid]} board", width=600, height=200 + 34 * len(body) + 120)
        paper = pygame.Rect(r.x + 30, r.y + 66, r.w - 60, r.h - 140)
        pygame.draw.rect(self.screen, WHITE, paper, border_radius=4)
        pygame.draw.circle(self.screen, VERMILLION, (paper.right - 30, paper.y + 30), 12)  # the seal
        self.text(self.f_small, "From " + lt.sender, (paper.x + 20, paper.y + 16), BLACK)
        self.text(self.f_title, lt.title, (paper.x + 20, paper.y + 40), BLACK)
        y = paper.y + 84
        for s in body:
            for s2 in wrap(self.f_body, s, paper.w - 40):
                self.text(self.f_body, s2, (paper.x + 20, y), BLACK)
                y += self.f_body.get_linesize()
        self.text(self.f_ital, lt.body[-1], (paper.right - 24, y + 8), BLACK, "topright")
        x, by = r.x + 30, r.bottom - 60
        if lt.receipt:
            w, h = self.f_body.size(lt.receipt)
            t = pygame.Rect(x, by, w + 32, h + 16)
            pygame.draw.rect(self.screen, BLACK, t, border_radius=6)
            pygame.draw.rect(self.screen, AMBER, t, 2, border_radius=6)
            self.text(self.f_body, lt.receipt, t.center, WHITE, "center")
            x = t.right + 12
        else:
            for label, action in lt.options:
                if action == "buy_lens":
                    ok = story.credits >= LENS_PRICE and not story.lens
                    label = f"Buy lens ({LENS_PRICE} cr)" if ok else f"Buy lens (need {LENS_PRICE} cr)"

                    def fn(a=action):
                        story.letter_action(sid, lt, a)
                else:
                    ok = True

                    def fn():
                        self.letter = None
                w, h = self.button_size(label)
                self.button((x, by, w, h), label, fn, "lbtn:" + action, enabled=ok)
                x += w + 12
        w, h = self.button_size("Back to board", self.f_small)
        self.button((r.right - w - 30, by + 4, w, h), "Back to board", lambda: setattr(self, "letter", None), "back",
                    light=False, f=self.f_small)

    def draw_pause(self):
        shade = pygame.Surface((self.W, self.H), pygame.SRCALPHA)
        shade.fill((*SPACE, 200))
        self.screen.blit(shade, (0, 0))
        self.hit(self.screen.get_rect(), lambda: None, "shade")
        lines = [
            "Fly: W thrust, S reverse, A/D turn, Space brake, Shift boost.",
            "Mine: hold M or left mouse next to pale ice rocks.",
            "Dock: fly into a station's pad. Undock: fly off.",
            "Talk: click a bubble or press Enter for the next line.",
            "R: tow back to Lantern Wharf.   F12: screenshot.",
            "",
            "Mode 1: dock talk + ship log. L or the Log icon opens the log.",
            "Mode 2: dock talk + leads board. L or the Leads icon opens it.",
            "Mode 3: letters on station boards + a chronicle of world changes.",
            "            B opens a board while docked, C the chronicle.",
        ]
        lh = self.f_body.get_linesize()
        r = pygame.Rect(0, 0, min(760, self.W - 40), 150 + lh * len(lines))
        r.center = (self.W // 2, self.H // 2)
        pygame.draw.rect(self.screen, PANEL, r, border_radius=10)
        pygame.draw.rect(self.screen, WHITE, r, 1, border_radius=10)
        self.text(self.f_title, "Paused", (r.x + 24, r.y + 18), WHITE)
        y = r.y + 64
        for s in lines:
            self.text(self.f_body, s, (r.x + 24, y), WHITE)
            y += lh
        x, y = r.x + 24, r.bottom - 58
        for label, fn, tag in [("Resume", self.toggle_pause, "resume"),
                               ("Mode 1", lambda: self.restart(1), "mode1"),
                               ("Mode 2", lambda: self.restart(2), "mode2"),
                               ("Mode 3", lambda: self.restart(3), "mode3"),
                               ("Quit", lambda: pygame.event.post(pygame.event.Event(pygame.QUIT)), "quit")]:
            w, h = self.button_size(label)
            self.button((x, y, w, h), label, fn, tag)
            x += w + 12


# ---------------------------------------------------------------- headless check
def selftest(app, shots=None):
    """Play the little story in all three modes without a window and check the rules."""
    dt = 1 / 60
    fails = []
    frame = [0]

    def check(ok, what):
        print(("PASS " if ok else "FAIL ") + what)
        if not ok:
            fails.append(what)

    def snap(name):
        app.draw()
        if shots:
            os.makedirs(shots, exist_ok=True)
            pygame.image.save(app.screen, os.path.join(shots, name + ".png"))

    def wallet():
        return app.story.credits, app.story.ice

    def run(seconds, **ctrl):
        before = wallet()
        for _ in range(int(round(seconds / dt))):
            app.step(dt, Controls(**ctrl))
            frame[0] += 1
            if frame[0] % 10 == 0:
                app.draw()
        if not ctrl.get("mine") and wallet() != before:
            check(False, f"credits/cargo changed with no player choice: {before} -> {wallet()}")

    def press(key):
        app.handle(pygame.event.Event(pygame.KEYDOWN, key=key, mod=0, unicode=""))

    def click_at(pos):
        app.handle(pygame.event.Event(pygame.MOUSEBUTTONDOWN, button=1, pos=pos))
        app.handle(pygame.event.Event(pygame.MOUSEBUTTONUP, button=1, pos=pos))

    def tags():
        app.draw()
        return [t for _, _, t in app.hits]

    def click(tag):
        """Click the UI element with this tag, the way a player would. Returns False if it isn't on screen."""
        app.draw()
        for rect, _, t in app.hits:
            if t == tag:
                before = wallet()
                click_at(rect.center)
                if not (tag.startswith("opt:") or tag.startswith("lbtn:")) and wallet() != before:
                    check(False, f"clicking {tag} changed credits/cargo")
                return True
        return False

    def talk_through(stop_at_choice=True):
        """Click through speech bubbles. Returns the speakers heard."""
        heard = []
        for _ in range(40):
            ln = app.story.line
            if ln is None or (ln.kind == "choice" and stop_at_choice):
                break
            heard.append((ln.who, ln.text))
            if not click("bubble"):
                break
        return heard

    def fly_to_dock(sid):
        p = world.pos(sid)
        app.ship.pos, app.ship.vel = p + V(0, -260), V()
        app.ship.angle = math.pi / 2
        app.state, app.at, app.armed = "free", None, True
        app.ship.auto_stop_v = None
        for _ in range(int(4 / dt)):
            app.step(dt, Controls(up=app.state == "free"))
            if app.state == "docked":
                break
        return app.state == "docked" and app.at == sid

    def mine_full():
        rock = next(r for r in app.world.rocks if r.ice)
        app.ship.pos, app.ship.vel = rock.pos + V(rock.r + 50, 0), V()
        app.step(dt, Controls())
        run(15, mine=True)

    # ---------------- flight, locked to the tank preset
    s = SETTINGS
    check((s.max_speed, s.accel, s.inertia, s.turn_rate, s.brake, s.auto_brake, s.auto_brake_time, s.boost, s.zoom,
           s.lead) == (300, 450, 0.40, 200, 350, True, 1.0, 2.0, 0.80, 0), "flight preset is locked to the tank setup")
    check(app.state == "docked" and app.at == "wharf", "starts docked at Lantern Wharf")
    check(app.story.credits == 260 and app.story.ice == 0, "starts with 260 cr and an empty hold")
    check("P or Esc: pause" in app.hints(), "pause hint is shown")
    first = app.story.line
    check(first and first.who == "ilse" and "Welcome" in first.text, "Ilse welcomes you in a bubble")
    check("bubble" in tags(), "the bubble is clickable")
    snap("01-mode1-welcome")
    heard = talk_through()
    check(len(heard) == 4 and app.story.line is None, f"click advances Ilse's four lines ({len(heard)})")
    check("kettle" in app.story.heard and "rimewater" in app.story.heard, "Ilse's lines add Kettle Dock and Rimewater")

    run(0.5, up=True)
    check(app.state == "free" and app.story.line is None, "W undocks by flying off")
    app.ship.pos = V(-30000, 0)  # open space for the flight checks
    run(1.5, up=True)
    sp = app.ship.vel.length()
    check(abs(sp - 300) < 2, f"max speed 300 ({sp:.0f})")
    run(0.5)
    mid = app.ship.vel.length()
    run(0.55)
    check(abs(mid - 150) < 10 and app.ship.vel.length() < 1, f"auto-brake stops in 1 s ({mid:.0f} at 0.5 s)")
    a0 = app.ship.angle
    run(0.45, left=True)
    check(abs(math.degrees(a0 - app.ship.angle) - 90) < 2, "A turns at 200 deg/s")
    run(3, up=True, boost=True)
    check(abs(app.ship.vel.length() - 600) < 3, f"Shift boosts x2 ({app.ship.vel.length():.0f})")
    run(1.2)
    run(1.5, down=True)
    check(app.ship.vel.length() > 200 and app.ship.vel.normalize().dot(app.ship.fwd) < -0.9, "S flies backward")
    run(1.2)
    run(1.0, brake=True)

    # ---------------- mining
    rock = next(r for r in app.world.rocks if r.ice)
    app.ship.pos, app.ship.vel = rock.pos + V(rock.r + 50, 0), V()
    app.step(dt, Controls())
    check("Hold M or left mouse: mine" in app.hints(), "near ice the literal mining hint shows")
    away = (app.W // 2 + 200, app.H // 2 + 150)
    app.handle(pygame.event.Event(pygame.MOUSEBUTTONDOWN, button=1, pos=away))
    for _ in range(int(1.05 / dt)):
        app.step(dt, app.controls())
    app.handle(pygame.event.Event(pygame.MOUSEBUTTONUP, button=1, pos=away))
    check(app.story.ice == 2, f"holding left mouse near ice mines ({app.story.ice} in 1 s)")
    snap("02-mining")
    while app.story.ice < 20 and app.time < 200:
        app.step(dt, Controls(mine=True))
    check(app.toast == "Hold full" and app.toast_t > 0, "'Hold full' shows the first time the hold fills")
    snap("03-hold-full")
    run(5, mine=True)
    check(app.story.ice == 20, f"hold caps at 20 ({app.story.ice})")
    again = Story(1)
    again.mine(11)
    again.events.clear()
    again.ice = 0  # test setup only
    again.mine(11)
    check(again.ice == 20 and not again.events, "'Hold full' is not shown a second time")
    check("rimewater" in app.story.visited, "flying into Rimewater marks it visited")

    # ---------------- Kettle Dock: deliver, receipt, ring
    check(fly_to_dock("kettle"), "flying into Kettle Dock docks")
    check(app.story.credits == 260 and app.story.ice == 20, "docking alone changes nothing")
    talk_through()
    ln = app.story.line
    opts = [o[1] for o in ln.options] if ln else []
    check(ln and ln.kind == "choice" and opts == ["deliver", "sell_kettle", "keep"], "Kettle offers Deliver / Sell / Keep")
    snap("04-kettle-choice")
    check(app.story.credits == 260 and app.story.ice == 20, "nothing changes while the choice waits")
    click("opt:deliver")
    check(app.story.line and app.story.line.text == "Gave 20 ice. Got 200 cr.", "receipt: 'Gave 20 ice. Got 200 cr.'")
    check(app.story.credits == 460 and app.story.ice == 0, "deliver pays 200 cr and empties the hold")
    check(app.story.ring_lit, "greenhouse ring lights up")
    snap("05-receipt")
    heard = talk_through()
    check(any(w == "tamsin" and "tender" in t for w, t in heard), "Tamsin explains tenders")
    run(5)
    check(app.ring_glow > 3.9, "the ring is visibly lit")
    snap("06-ring-lit")
    run(0.4, up=True)
    log = app.story.log
    check(bool(log) and log[-1].text() == "Kettle Dock: gave Odo 20 ice, got 200 cr. Tamsin explained tenders.",
          f"log line: {log[-1].text() if log else None}")

    # ---------------- the log
    check(click("icon:log") and app.panel == "log", "Log icon opens the ship log")
    snap("07-log")
    click("log_row:0")
    check(app.replay is not None and app.replay[0].station == "kettle", "clicking a log line replays that visit")
    n = len(app.replay[0].lines)
    snap("08-replay")
    for _ in range(n):
        click("replay")
    check(app.replay is None and app.panel == "log", f"replay steps through {n} bubbles and returns to the log")
    click("close")
    check(app.panel is None, "Close shuts the log")

    # ---------------- back at the Wharf: Cinder Hook lead and Orrin's lens
    press(pygame.K_r)
    run(0.6)
    check(app.state == "docked" and app.at == "wharf", "R tows back to the Wharf")
    heard = talk_through()
    check(any("lantern current" in t for _, t in heard), "Ilse mentions the lantern current to Cinder Hook")
    ln = app.story.line
    check(ln and ln.who == "orrin" and "half again as fast" in ln.text, "Orrin offers the lens")
    buy = [o for o in ln.options if o[1] == "buy_lens"] if ln else []
    check(buy and not buy[0][2], "can't buy the lens at 460 cr")
    snap("09-orrin")
    click("opt:not_now")
    talk_through()
    pins = " ".join(t for _, t in app.story.pinned())
    check("lens" in pins and "Cinder Hook" in pins, "log pins the lens offer and the Cinder Hook lead")
    app.open_panel("log")
    snap("10-log-pinned")
    app.close_panel()

    # ---------------- the lantern current and Cinder Hook
    run(0.4, up=True)
    app.ship.pos, app.ship.vel = V(world.CURRENT[1]), V()
    x0 = app.ship.pos.x
    run(3)
    check(app.in_current and app.ship.pos.x - x0 > 200, f"the lantern current drifts you east ({app.ship.pos.x - x0:.0f})")
    snap("11-current")
    check(fly_to_dock("hook"), "Cinder Hook docks")
    talk_through()
    check(app.story.lamp_lit, "Rook lights the lamp")
    run(0.4, up=True)

    # ---------------- pause
    press(pygame.K_p)
    p0 = V(app.ship.pos)
    run(1.0, up=True)
    check(app.paused and app.ship.pos == p0, "P pauses")
    snap("12-pause")
    press(pygame.K_ESCAPE)
    check(not app.paused and "P or Esc: pause" not in app.hints(), "Esc resumes and the pause hint is gone")

    # ---------------- mode 2: leads board
    press(pygame.K_F2)
    check(app.mode == 2 and app.story.credits == 260 and app.story.ice == 0 and not app.story.log,
          "F2 restarts the story in mode 2")
    t = tags()
    check("icon:leads" in t and "icon:log" not in t, "mode 2 has a Leads icon and no log")
    talk_through()
    press(pygame.K_l)
    leads = {n: (how, who, been) for n, how, who, been in app.story.leads()}
    check(app.panel == "leads" and leads.get("Kettle Dock", (0, 0, True))[2] is False, "Kettle Dock is 'heard of'")
    check(leads.get("Kettle Dock", ("", ""))[1] == "Ilse", "the card says who told you")
    snap("13-leads")
    app.close_panel()
    run(0.4, up=True)
    fly_to_dock("kettle")
    talk_through()
    run(0.4, up=True)
    press(pygame.K_r)
    run(0.6)
    talk_through()
    click("opt:not_now")
    talk_through()
    leads = {n: been for n, _, _, been in app.story.leads()}
    check(leads.get("Kettle Dock") is True and leads.get("Cinder Hook") is False, "visited = 'been there'")
    app.open_panel("leads")
    snap("14-leads-later")
    app.close_panel()

    # ---------------- mode 3: letters and the chronicle
    press(pygame.K_F3)
    check(app.mode == 3 and app.story.credits == 260, "F3 restarts the story in mode 3")
    t = tags()
    check("icon:chronicle" in t and "board" not in t, "mode 3 has a Chronicle icon, no board yet")
    talk_through()
    run(0.4, up=True)
    mine_full()
    fly_to_dock("kettle")
    talk_through()
    click("opt:deliver")
    talk_through()
    check(app.story.chronicle == ["The greenhouse ring at Kettle Dock is lit."], "the ring gets one Chronicle line")
    snap("15-mode3-kettle")
    run(0.4, up=True)
    press(pygame.K_r)
    run(0.6)
    heard = talk_through()
    check(not any(w == "orrin" for w, _ in heard), "Orrin does not speak in mode 3")
    check(any("ring lit up" in tx for _, tx in heard), "Ilse refers back to the ring")
    keys = [lt.key for lt in app.story.board("wharf")]
    check("lens" in keys and "odo_thanks" in keys, f"the Wharf board holds the lens letter and Odo's thanks {keys}")
    check(click("board") and app.panel == "board", "the board opens from the station")
    snap("16-board")
    li = keys.index("lens")
    click(f"letter:{li}")
    check(app.letter is not None and app.letter.key == "lens", "the lens letter opens")
    snap("17-letter")
    check(not click("lbtn:buy_lens") and not app.story.lens, "can't buy the lens at 460 cr from the letter")
    app.story.credits = 700  # test setup only: skip a second mining run
    click("lbtn:buy_lens")
    check(app.story.lens and app.story.credits == 100, "the letter's Buy button buys the lens")
    check(app.letter.receipt == "Paid 600 cr. Lens fitted.", "the letter shows a receipt")
    snap("18-letter-bought")
    check(abs(app.story.mine_rate - 3.0) < 1e-9, "the lens mines half again as fast")
    app.close_panel()
    check(click("icon:chronicle") and app.panel == "chronicle", "Chronicle icon opens the chronicle")
    snap("19-chronicle")
    app.close_panel()

    # ---------------- undocking mid-talk says nothing is lost
    press(pygame.K_F1)
    run(0.4, up=True)
    check(app.story.line is None and "welcome" not in app.story.done, "undocking stops the bubbles mid-talk")
    press(pygame.K_r)
    run(0.6)
    check(app.story.line and "Welcome" in app.story.line.text, "the unsaid welcome plays at the next docking")

    print("FAIL" if fails else "PASS", f"({len(fails)} failed)")
    return 1 if fails else 0


def main():
    test = "--selftest" in sys.argv
    shots = None
    if "--shots" in sys.argv:
        i = sys.argv.index("--shots")
        shots = sys.argv[i + 1] if i + 1 < len(sys.argv) else os.path.join(SHOT_DIR, "selftest")
    if test:
        os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
    pygame.display.init()
    pygame.font.init()
    screen = pygame.display.set_mode(SIZE, 0 if test else pygame.RESIZABLE)
    pygame.display.set_caption("Messaging lab")
    app = App(screen)
    code = selftest(app, shots) if test else app.run()
    pygame.quit()
    sys.exit(code or 0)


if __name__ == "__main__":
    main()
