# /// script
# requires-python = ">=3.10"
# dependencies = ["pygame-ce>=2.5.6"]
# ///
"""Flight lab: a top-down sandbox for trying ship flight models.

Run:   uv run flight_lab.py        (or: python flight_lab.py after pip install -r requirements.txt)
Check: python flight_lab.py --selftest   (headless, prints PASS/FAIL)
"""
import math
import os
import random
import sys
import time
from collections import deque

import pygame
from pygame.math import Vector2 as V

from flight import HINTS, SCHEMES, Controls, Pilot, Settings, Ship, fly, list_presets, load_preset, save_preset
from palette import (BLACK, BLUE, GREEN, ICE_FILL, ORANGE, PAD_FILL, ROCK_FILL, SKY, SPACE, VERMILLION, WHITE,
                     YELLOW)
from panel import Panel, WIDTH as PANEL_W
from world import ARRIVE_R, DOCK_R, MARKERS, RING_R, STATION, World

HERE = os.path.dirname(os.path.abspath(__file__))
PRESET_DIR = os.path.join(HERE, "presets")
SHOT_DIR = os.path.join(HERE, "screenshots")
DEFAULT_PRESET = "light drift"
SIZE = (1280, 800)
SHIP_R = 17

# ship outline, nose along +x (copied from Proto 1's sketchy hull)
SS = 1.25
HULL = [(24, 0), (12, -9), (-4, -12), (-14, -11), (-18, -5), (-18, 5), (-14, 11), (-4, 12), (12, 9)]
POD_L = [(4, -10), (-10, -19), (-18, -18), (-15, -9)]
POD_R = [(x, -y) for x, y in POD_L]
STRIPE = [(-7, -11.4), (-2, -11.9), (-2, 11.9), (-7, 11.4)]
NOZZLE = [(-17, -5), (-23, -6.5), (-23, 6.5), (-17, 5)]
HULL, POD_L, POD_R, STRIPE, NOZZLE = ([(x * SS, y * SS) for x, y in p] for p in (HULL, POD_L, POD_R, STRIPE, NOZZLE))

# parallax, cell size, stars per cell, color, radius
STAR_LAYERS = [(0.15, 360, 3, BLUE, 1), (0.45, 360, 3, SKY, 1), (1.0, 300, 2, WHITE, 1.5)]


class App:
    def __init__(self, screen):
        self.screen = screen
        self.W, self.H = screen.get_size()
        self.world = World()
        self.ship = Ship(STATION)
        self.pilot = Pilot()
        self.settings = Settings()
        self.preset_name, self.preset_dirty = "defaults", False
        self.presets = list_presets(PRESET_DIR)
        for name, path in self.presets:
            if name == DEFAULT_PRESET:
                self.load_preset(path)
        self.state = "docked"
        self.dock_armed = False
        self.dock_t, self.dock_from = 0.0, V()
        self.cam = V(STATION)
        self.zoom = self.settings.zoom
        self.time = 0.0
        self.trail = deque(maxlen=160)
        self.trail_t = 0.0
        self.trip_t0, self.trip_from, self.trip_dist = 0.0, "Home", 0.0
        self.last_reached = None
        self.msg, self.msg_t = "Docked at Home. Fly off to undock.", 4.0
        self.flame = 0.0
        self.mouse_screen = None
        self.lmb = self.rmb = False
        self.stars = {}
        self.hud_rect = pygame.Rect(0, 0, 0, 0)
        self.f_hud = pygame.font.Font(None, 22)
        self.f_big = pygame.font.Font(None, 40)
        self.f_label = pygame.font.Font(None, 24)
        self.panel = Panel(self)

    # ---- settings and presets
    def settings_changed(self):
        self.preset_dirty = True

    def set_scheme(self, n):
        if n in SCHEMES and n != self.pilot.scheme:
            self.pilot.set_scheme(n)
            self.flash(f"Scheme {n}: {SCHEMES[n]}")

    def load_preset(self, path):
        try:
            self.preset_name, self.settings = load_preset(path)
        except (OSError, ValueError, TypeError) as err:
            self.flash(f"Could not load preset: {err}")
            return
        self.preset_dirty = False
        self.flash(f"Preset: {self.preset_name}")

    def save_preset(self, name):
        path = save_preset(PRESET_DIR, name, self.settings)
        self.preset_name, self.preset_dirty = name.strip(), False
        self.presets = list_presets(PRESET_DIR)
        self.flash(f"Saved preset: {self.preset_name}")
        return path

    def flash(self, s, t=2.5):
        self.msg, self.msg_t = s, t

    # ---- simulation
    def go_home(self):
        self.ship.pos, self.ship.vel = V(STATION), V()
        self.state, self.dock_armed = "docked", False
        self.pilot.target = None
        self.cam = V(STATION)
        self.trail.clear()
        self.flash("Towed home")

    def step(self, dt, c):
        self.time += dt
        ship, s = self.ship, self.settings
        thrust, speed, brake = self.pilot.steer(ship, s, c, dt)
        wants_out = thrust is not None and speed > 0
        if self.state == "docked":
            ship.pos, ship.vel = V(STATION), V()
            if wants_out:
                self.state, self.dock_armed = "free", False
                self.trip_t0, self.trip_from, self.trip_dist = self.time, "Home", 0.0
                self.last_reached = None
                self.flash("Undocked", 1.2)
            else:
                ship.thrusting = ship.retro = False
        elif self.state == "docking":
            self.dock_t = min(1.0, self.dock_t + dt / 0.4)
            k = self.dock_t * self.dock_t * (3 - 2 * self.dock_t)
            ship.pos, ship.vel, ship.thrusting = self.dock_from.lerp(STATION, k), V(), False
            ship.retro = False
            if self.dock_t >= 1:
                self.state = "docked"
        if self.state == "free":
            fly(ship, s, thrust, speed, brake, c.boost, dt)
            if self.world.collide(ship, SHIP_R) > 150:
                self.flash("Bump!", 0.8)
            self.trip_dist += ship.vel.length() * dt
            self.check_arrivals()
        self.flame += ((1.0 if ship.thrusting else 0.0) - self.flame) * min(1.0, dt * 12)
        self.trail_t += dt
        if self.trail_t > 0.05:
            self.trail_t = 0.0
            if not self.trail or (self.trail[-1] - ship.pos).length() > 3:
                self.trail.append(V(ship.pos))
        lead = ship.vel * s.lead
        max_lead = 0.4 * min(self.W, self.H) / max(self.zoom, 0.05)
        if lead.length() > max_lead:
            lead.scale_to_length(max_lead)
        self.cam += (ship.pos + lead - self.cam) * (1 - math.exp(-6 * dt))
        self.zoom += (s.zoom - self.zoom) * (1 - math.exp(-8 * dt))
        self.msg_t -= dt

    def check_arrivals(self):
        ship = self.ship
        home = (ship.pos - STATION).length()
        if not self.dock_armed and home > DOCK_R + 120:
            self.dock_armed = True
        if self.dock_armed and home < DOCK_R:
            self.state, self.dock_t, self.dock_from = "docking", 0.0, V(ship.pos)
            self.pilot.target = None
            self.flash(f"Docked at Home. {self.trip_text()}", 4.0)
            return
        for name, p, _ in MARKERS:
            if name != self.last_reached and (ship.pos - p).length() < ARRIVE_R:
                self.flash(f"Reached {name}. {self.trip_text()}", 5.0)
                self.last_reached = name
                self.trip_t0, self.trip_from, self.trip_dist = self.time, name, 0.0

    def trip_text(self):
        t = max(0.001, self.time - self.trip_t0)
        return f"From {self.trip_from}: {self.trip_dist / 1000:.1f}k in {t:.1f} s (avg {self.trip_dist / t:.0f} px/s)"

    # ---- input
    def read_controls(self, lmb_click, rmb_click):
        k = pygame.key.get_pressed() if not self.panel.focus else None

        def held(*keys):
            return bool(k) and any(k[x] for x in keys)

        mouse = None
        if self.mouse_screen is not None and not self.panel.contains(self.mouse_screen):
            mouse = self.s2w(self.mouse_screen)
        return Controls(
            up=held(pygame.K_w, pygame.K_UP), down=held(pygame.K_s, pygame.K_DOWN),
            left=held(pygame.K_a, pygame.K_LEFT), right=held(pygame.K_d, pygame.K_RIGHT),
            boost=held(pygame.K_LSHIFT, pygame.K_RSHIFT), brake=held(pygame.K_SPACE),
            mouse=mouse, lmb=self.lmb or lmb_click, rmb=self.rmb, rmb_click=rmb_click)

    def run(self):
        clock = pygame.time.Clock()
        pygame.key.stop_text_input()
        while True:
            dt = min(clock.tick(120) / 1000.0, 1 / 30)
            lmb_click = rmb_click = False  # so a click shorter than a frame still counts
            for e in pygame.event.get():
                if e.type == pygame.QUIT:
                    return
                if e.type in (pygame.VIDEORESIZE, pygame.WINDOWSIZECHANGED):
                    self.screen = pygame.display.get_surface()
                    self.W, self.H = self.screen.get_size()
                if e.type in (pygame.MOUSEMOTION, pygame.MOUSEBUTTONDOWN):
                    self.mouse_screen = e.pos
                elif e.type == pygame.WINDOWLEAVE:
                    self.mouse_screen = None
                if self.panel.visible and self.panel.handle(e):
                    continue
                if e.type == pygame.KEYDOWN:
                    if e.key == pygame.K_ESCAPE:
                        return
                    if e.key == pygame.K_TAB:
                        self.panel.visible = not self.panel.visible
                    elif pygame.K_1 <= e.key <= pygame.K_5:
                        self.set_scheme(e.key - pygame.K_0)
                    elif e.key in (pygame.K_F12, pygame.K_p):
                        self.screenshot()
                    elif e.key == pygame.K_r:
                        self.go_home()
                elif e.type == pygame.MOUSEWHEEL:
                    self.settings.zoom = max(0.2, min(2.0, self.settings.zoom * 1.1 ** e.y))
                    self.settings_changed()
                elif e.type == pygame.MOUSEBUTTONDOWN:
                    if e.button == 1:
                        self.lmb = lmb_click = True
                    elif e.button == 3:
                        self.rmb = rmb_click = True
                elif e.type == pygame.MOUSEBUTTONUP:
                    if e.button == 1:
                        self.lmb = False
                    elif e.button == 3:
                        self.rmb = False
            if pygame.mouse.get_focused():
                self.mouse_screen = pygame.mouse.get_pos()
            self.step(dt, self.read_controls(lmb_click, rmb_click))
            self.draw()
            pygame.display.flip()

    def screenshot(self):
        os.makedirs(SHOT_DIR, exist_ok=True)
        name = time.strftime("flight-lab-%Y%m%d-%H%M%S.png")
        pygame.image.save(self.screen, os.path.join(SHOT_DIR, name))
        self.flash(f"Saved screenshots/{name}")

    # ---- view helpers
    def w2s(self, p):
        return ((p[0] - self.cam.x) * self.zoom + self.W / 2, (p[1] - self.cam.y) * self.zoom + self.H / 2)

    def s2w(self, p):
        return V((p[0] - self.W / 2) / self.zoom + self.cam.x, (p[1] - self.H / 2) / self.zoom + self.cam.y)

    def xform(self, pts, pos, ang):
        c, s = math.cos(ang), math.sin(ang)
        z, ox, oy = self.zoom, self.W / 2 - self.cam.x * self.zoom, self.H / 2 - self.cam.y * self.zoom
        return [((pos[0] + x * c - y * s) * z + ox, (pos[1] + x * s + y * c) * z + oy) for x, y in pts]

    def poly(self, pts, pos, ang, color, w=2):
        sp = self.xform(pts, pos, ang)
        pygame.draw.polygon(self.screen, color, sp)
        if w:
            pygame.draw.polygon(self.screen, BLACK, sp, w)

    def text(self, font, s, pos, color=WHITE, anchor="topleft"):
        img = font.render(s, True, color)
        r = img.get_rect(**{anchor: (int(pos[0]), int(pos[1]))})
        self.screen.blit(img, r)
        return r

    def boxed_lines(self, lines, pos, anchor, font, border=WHITE):
        imgs = [font.render(s, True, WHITE) for s in lines]
        w = max(i.get_width() for i in imgs) + 20
        h = sum(i.get_height() + 3 for i in imgs) + 14
        r = pygame.Rect(0, 0, w, h)
        setattr(r, anchor, pos)
        pygame.draw.rect(self.screen, BLACK, r)
        pygame.draw.rect(self.screen, border, r, 1)
        y = r.y + 8
        for img in imgs:
            self.screen.blit(img, (r.x + 10, y))
            y += img.get_height() + 3
        return r

    # ---- drawing
    def draw(self):
        self.screen.fill(SPACE)
        self.draw_stars()
        self.draw_station()
        self.draw_markers()
        self.draw_rocks()
        self.draw_trail()
        self.draw_target()
        self.draw_ship()
        self.draw_hud()
        self.draw_edge_markers()
        self.panel.draw(self.screen)

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

    def draw_station(self):
        z = self.zoom
        c = self.w2s(STATION)
        if not -RING_R * z - 50 < c[0] < self.W + RING_R * z + 50 or not -RING_R * z - 50 < c[1] < self.H + RING_R * z + 50:
            return
        spin = self.time * 0.15
        for i in range(4):
            a = spin + i * math.tau / 4
            p = (STATION[0] + math.cos(a) * RING_R, STATION[1] + math.sin(a) * RING_R)
            pygame.draw.line(self.screen, GREEN, self.w2s(STATION), self.w2s(p), max(2, int(8 * z)))
        pygame.draw.circle(self.screen, GREEN, c, RING_R * z, max(2, int(10 * z)))
        for i in range(6):
            a = spin + (i + 0.5) * math.tau / 6
            p = self.w2s((math.cos(a) * RING_R, math.sin(a) * RING_R))
            pygame.draw.circle(self.screen, BLUE, p, 26 * z)
            pygame.draw.circle(self.screen, WHITE, p, 26 * z, max(1, int(3 * z)))
        pygame.draw.circle(self.screen, PAD_FILL, c, DOCK_R * z)
        for i in range(12):  # dashed dock ring
            a0 = -self.time * 0.8 + i * math.tau / 12
            pts = [self.w2s((math.cos(a0 + t * 0.3) * DOCK_R, math.sin(a0 + t * 0.3) * DOCK_R)) for t in range(4)]
            pygame.draw.lines(self.screen, SKY, False, pts, max(2, int(4 * z)))
        label = "DOCKED" if self.state in ("docked", "docking") else "HOME: fly in to dock"
        self.text(self.f_label, label, (c[0], c[1] - (RING_R + 40) * z), WHITE, "center")

    def draw_markers(self):
        z = self.zoom
        for name, p, color in MARKERS:
            sp = self.w2s(p)
            if not (-200 < sp[0] < self.W + 200 and -200 < sp[1] < self.H + 200):
                continue
            r = 40 * z
            diamond = [(sp[0] + r, sp[1]), (sp[0], sp[1] + r), (sp[0] - r, sp[1]), (sp[0], sp[1] - r)]
            pygame.draw.polygon(self.screen, color, diamond)
            pygame.draw.polygon(self.screen, WHITE, diamond, 2)
            pulse = ARRIVE_R * z * (0.8 + 0.2 * math.sin(self.time * 3))
            pygame.draw.circle(self.screen, color, sp, pulse, 2)
            self.text(self.f_label, name, (sp[0], sp[1] - r - 16), WHITE, "center")

    def draw_rocks(self):
        z = self.zoom
        a = self.s2w((-40, -40))
        b = self.s2w((self.W + 40, self.H + 40))
        lw = 2 if z > 0.5 else 1
        for rock in self.world.in_rect(a.x, a.y, b.x, b.y):
            sp = self.w2s(rock.pos)
            rr = rock.r * z
            if not (-rr < sp[0] < self.W + rr and -rr < sp[1] < self.H + rr):
                continue
            fill, edge = (ICE_FILL, SKY) if rock.ice else (ROCK_FILL, ORANGE)
            if rr < 4:
                pygame.draw.circle(self.screen, edge, sp, max(1.0, rr))
                continue
            pts = self.xform(rock.pts, rock.pos, rock.a0 + rock.spin * self.time)
            pygame.draw.polygon(self.screen, fill, pts)
            pygame.draw.polygon(self.screen, edge, pts, lw)

    def draw_trail(self):
        n = len(self.trail)
        for i, p in enumerate(self.trail):
            pygame.draw.circle(self.screen, SKY, self.w2s(p), max(1.0, (1 + 2.5 * i / n) * min(1.0, self.zoom + 0.3)))

    def draw_target(self):
        t = self.pilot.target
        if self.pilot.scheme == 4 and t is not None:
            sp = self.w2s(t)
            pygame.draw.circle(self.screen, YELLOW, sp, 14, 2)
            pygame.draw.line(self.screen, YELLOW, (sp[0] - 20, sp[1]), (sp[0] + 20, sp[1]), 2)
            pygame.draw.line(self.screen, YELLOW, (sp[0], sp[1] - 20), (sp[0], sp[1] + 20), 2)
        if self.pilot.scheme in (2, 5) and self.mouse_screen and not self.panel.contains(self.mouse_screen):
            pygame.draw.circle(self.screen, WHITE, self.mouse_screen, 10, 2)

    def draw_ship(self):
        ship = self.ship
        pos, ang = ship.pos, ship.angle
        if self.flame > 0.05:
            boosting = pygame.key.get_mods() & pygame.KMOD_SHIFT
            L = (8 + 16 * self.flame * (1.6 if boosting else 1.0) + 4 * abs(math.sin(self.time * 37))) * SS
            if ship.retro:
                nose = 26 * SS
                self.poly([(nose, -3.5 * SS), (nose + L * 0.55, 0), (nose, 3.5 * SS)], pos, ang, VERMILLION)
                self.poly([(nose, -1.8 * SS), (nose + L * 0.3, 0), (nose, 1.8 * SS)], pos, ang, YELLOW, w=0)
            else:
                back = -23 * SS
                self.poly([(back, -5 * SS), (back - L, 0), (back, 5 * SS)], pos, ang, VERMILLION)
                self.poly([(back, -2.5 * SS), (back - L * 0.55, 0), (back, 2.5 * SS)], pos, ang, YELLOW, w=0)
        self.poly(POD_L, pos, ang, ORANGE)
        self.poly(POD_R, pos, ang, ORANGE)
        self.poly(NOZZLE, pos, ang, BLUE)
        self.poly(HULL, pos, ang, WHITE)
        self.poly(STRIPE, pos, ang, VERMILLION)
        cp = self.w2s(pos + ship.fwd * (9 * SS))
        pygame.draw.circle(self.screen, SKY, cp, 6 * SS * self.zoom)
        pygame.draw.circle(self.screen, BLACK, cp, 6 * SS * self.zoom, max(1, round(2 * self.zoom)))

    def draw_edge_markers(self):
        left = (PANEL_W + 30) if self.panel.visible else 40
        top, right, bottom = 40, self.W - 40, self.H - 70
        cx, cy = self.W / 2, self.H / 2
        for name, p, color in [("Home", STATION, GREEN)] + MARKERS:
            sx, sy = self.w2s(p)
            if left <= sx <= right and top <= sy <= bottom:
                continue
            dx, dy = sx - cx, sy - cy
            kx = ((right - cx) if dx > 0 else (cx - left)) / abs(dx) if dx else 1e9
            ky = ((bottom - cy) if dy > 0 else (cy - top)) / abs(dy) if dy else 1e9
            k = min(kx, ky)
            ex, ey = cx + dx * k, cy + dy * k
            if self.hud_rect.inflate(30, 30).collidepoint(ex, ey):
                ey = self.hud_rect.bottom + 30  # slide below the readout box
            a = math.atan2(dy, dx)
            tri = [(ex + math.cos(a + o) * s, ey + math.sin(a + o) * s) for o, s in ((0, 16), (2.5, 14), (-2.5, 14))]
            pygame.draw.polygon(self.screen, color, tri)
            pygame.draw.polygon(self.screen, WHITE, tri, 2)
            img = self.f_hud.render(f"{name} {(p - self.ship.pos).length() / 1000:.1f}k", True, WHITE)
            r = img.get_rect(center=(ex - math.cos(a) * (img.get_width() / 2 + 22),
                                     ey - math.sin(a) * (img.get_height() / 2 + 20)))
            r.clamp_ip(self.screen.get_rect().inflate(-8, -8))
            self.screen.blit(img, r)

    def draw_hud(self):
        s, ship = self.settings, self.ship
        speed = ship.vel.length()
        where = "docked at Home" if self.state != "free" else self.trip_text()
        lines = [
            f"Scheme {self.pilot.scheme}: {SCHEMES[self.pilot.scheme]}",
            f"Preset: {self.preset_name}{' (edited)' if self.preset_dirty else ''}",
            f"Max speed {s.max_speed:.0f}   accel {s.accel:.0f}   inertia {s.inertia:.2f}",
            f"Turn {s.turn_rate:.0f}°/s   brake {s.brake:.0f}   auto-brake "
            + (f"{s.auto_brake_strength:.0f}" if s.auto_brake else "off"),
            f"Boost x{s.boost:.2f}   zoom {s.zoom:.2f}   lead {s.lead:.2f} s",
            f"Course hold {'on' if s.course_hold else 'off'}",
            where,
        ]
        r = self.boxed_lines(lines, (self.W - 12, 12), "topright", self.f_hud)
        self.text(self.f_big, f"{speed:4.0f} px/s", (r.right - 10, r.bottom + 8), WHITE, "topright")
        bar = pygame.Rect(r.right - 200, r.bottom + 44, 190, 8)
        cap = s.max_speed * max(1.0, s.boost)
        pygame.draw.rect(self.screen, BLACK, bar)
        pygame.draw.rect(self.screen, SKY, (bar.x, bar.y, int(bar.w * min(1.0, speed / cap)), bar.h))
        mx = bar.x + bar.w * s.max_speed / cap
        pygame.draw.line(self.screen, YELLOW, (mx, bar.y - 3), (mx, bar.bottom + 3), 2)
        pygame.draw.rect(self.screen, WHITE, bar, 1)
        self.hud_rect = r.union(bar)
        hint = HINTS[self.pilot.scheme] + ".  1-5 scheme, Tab panel, wheel zoom, R home, F12/P screenshot, Esc quit"
        self.boxed_lines([hint], (self.W // 2, self.H - 10), "midbottom", self.f_hud)
        if self.msg_t > 0:
            self.boxed_lines([self.msg], (self.W // 2 + (PANEL_W // 2 if self.panel.visible else 0), 14), "midtop",
                             self.f_label, border=YELLOW)


# ---------------------------------------------------------------- headless check
def selftest(app):
    """Drive the lab through every scheme without a window and check the basics."""
    import tempfile

    dt = 1 / 60
    fails = []

    def check(ok, what):
        print(("PASS " if ok else "FAIL ") + what)
        if not ok:
            fails.append(what)

    def run(seconds, **ctrl):
        for _ in range(int(seconds / dt)):
            app.step(dt, Controls(**ctrl))
            app.draw()

    check(len(app.world.rocks) > 800, f"field has {len(app.world.rocks)} asteroids")
    check(app.preset_name == DEFAULT_PRESET, f"starts on preset '{app.preset_name}'")
    check(app.settings.max_speed >= 396, f"default max speed {app.settings.max_speed:.0f} is ~20% above 330")
    check(app.state == "docked", "starts docked")

    app.set_scheme(1)
    run(1.0, up=True)
    check(app.state == "free" and app.ship.pos.y < -150, "WASD: W undocks just by flying off")
    run(1.0, up=True, right=True)
    v = app.ship.vel
    check(abs(math.degrees(math.atan2(v.y, v.x)) + 45) < 5, f"WASD: W+D flies diagonally ({v.x:.0f}, {v.y:.0f})")
    run(0.08, up=True, right=True)
    run(0.05, up=True)
    run(0.08, up=True, left=True)
    run(0.5, up=True)
    a = math.degrees(app.ship.angle)
    check(3 < abs(a + 90) < 40, f"WASD: tapping a side key nudges and holds the course ({a:.0f} deg)")
    run(0.6)
    check(app.ship.vel.length() < 5, f"WASD: light inertia coasts to a stop ({app.ship.vel.length():.1f} px/s)")

    app.set_scheme(2)
    start = V(app.ship.pos)
    for _ in range(90):
        app.step(dt, Controls(up=True, mouse=app.ship.pos + V(1000, 0)))
    check(app.ship.pos.x - start.x > 200 and abs(math.cos(app.ship.angle) - 1) < 0.01, "mouse-aim: faces cursor and W thrusts")
    run(1.0, down=True)
    check(app.ship.vel.length() < 1, "mouse-aim: S brakes")

    app.set_scheme(3)
    a0 = app.ship.angle
    run(0.3, left=True)
    check(app.ship.angle < a0 - 1.0, "tank: A rotates")
    run(0.5, up=True)
    check(app.ship.vel.length() > 200 and app.ship.vel.normalize().dot(app.ship.fwd) > 0.95, "tank: W thrusts forward")
    run(1.2, down=True)
    check(app.ship.vel.length() > 200 and app.ship.vel.normalize().dot(app.ship.fwd) < -0.9,
          "tank: holding S flies backward")

    app.set_scheme(5)
    far_up = app.ship.pos + V(0, -100000)
    run(1.0, brake=True, mouse=far_up)
    run(0.6, right=True, mouse=far_up)
    v = app.ship.vel
    check(v.x > 200 and abs(v.y) < 60, f"hybrid: faces up, D strafes right ({v.x:.0f}, {v.y:.0f})")

    app.set_scheme(4)
    goal = app.ship.pos + V(-900, 300)
    app.step(dt, Controls(lmb=True, mouse=goal))
    for _ in range(int(6 / dt)):
        app.step(dt, Controls())
    miss = (app.ship.pos - goal).length()
    check(miss < 12 and app.pilot.target is None, f"click: flies to the click and stops ({miss:.0f} px off)")
    app.step(dt, Controls(lmb=True, mouse=V(STATION)))
    for _ in range(int(15 / dt)):
        app.step(dt, Controls())
    check(app.state == "docked", "click: clicking the station flies in and docks")
    run(0.2, up=True)
    check(app.state == "free", "undocks again with WASD")

    for name in ("snappy", "heavy hauler", "newtonian"):
        match = [p for n, p in app.presets if n == name]
        check(bool(match), f"starter preset '{name}' exists")
        if match:
            app.load_preset(match[0])
    app.settings.inertia = 1.0
    app.settings.auto_brake = False
    app.set_scheme(3)
    run(0.5, up=True)
    v0 = app.ship.vel.length()
    run(1.0)
    check(v0 > 100 and abs(app.ship.vel.length() - v0) < 1, "inertia 1: coasts forever")
    app.settings.auto_brake = True
    app.settings.auto_brake_strength = 200
    before = app.ship.vel.length()
    run(1.0)
    slowed = before - app.ship.vel.length()
    check(150 < slowed < 250, f"auto-brake strength 200 drops about 200 px/s in one second ({slowed:.0f})")
    app.settings.inertia = 0.4
    app.settings.auto_brake = True
    app.settings.auto_brake_strength = 0
    run(0.4, up=True)
    held = app.ship.vel.length()
    run(1.0)
    check(held > 80 and abs(app.ship.vel.length() - held) < 5,
          f"auto-brake 0 coasts instead of the inertia stop ({held:.0f} -> {app.ship.vel.length():.0f})")

    with tempfile.TemporaryDirectory() as tmp:
        app.settings.turn_rate = 123
        app.settings.inertia = 1.0
        app.settings.auto_brake_strength = 250
        path = save_preset(tmp, "My Test Ship", app.settings)
        name, s2 = load_preset(path)
        check(name == "My Test Ship" and s2.turn_rate == 123 and s2.inertia == 1.0 and s2.auto_brake_strength == 250,
              "preset save/load round trip")
        app.screen.fill(BLACK)
        app.draw()
        pygame.image.save(app.screen, os.path.join(tmp, "shot.png"))
        check(os.path.getsize(os.path.join(tmp, "shot.png")) > 1000, "renders a frame")

    print("FAIL" if fails else "PASS", f"({len(fails)} failed)")
    return 1 if fails else 0


def main():
    test = "--selftest" in sys.argv
    if test:
        os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
    pygame.display.init()
    pygame.font.init()
    flags = 0 if test else pygame.RESIZABLE
    screen = pygame.display.set_mode(SIZE, flags)
    pygame.display.set_caption("Flight lab")
    app = App(screen)
    code = selftest(app) if test else app.run()
    pygame.quit()
    sys.exit(code or 0)


if __name__ == "__main__":
    main()
