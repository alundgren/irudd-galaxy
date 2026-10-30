"""Mouse-driven tuning panel: schemes, knobs, toggles, presets. Tab shows or hides it."""
import os

import pygame

from flight import KNOBS, LIMITS, SCHEMES, SHORT, TOGGLES
from palette import BLACK, ORANGE, SKY, WHITE, YELLOW

PAD = 12
WIDTH = 340


class Panel:
    def __init__(self, app):
        self.app = app
        self.visible = True
        self.font = pygame.font.Font(None, 22)
        self.small = pygame.font.Font(None, 19)
        self.title = pygame.font.Font(None, 26)
        self.dragging = None  # knob key being dragged
        self.focus = False  # typing a preset name
        self.name = ""
        self.status = ""
        self.items = []  # (kind, rect, payload), rebuilt by layout()
        self.rect = pygame.Rect(0, 0, 0, 0)
        self.layout()

    # ---- layout
    def layout(self):
        x, y, w = PAD, PAD, WIDTH - 2 * PAD
        items = [("title", pygame.Rect(x, y, w, 22), None)]
        y += 30
        bw = (w - 4 * 4) // 5
        for i, n in enumerate(SCHEMES):
            items.append(("scheme", pygame.Rect(x + i * (bw + 4), y, bw, 26), n))
        y += 32
        items.append(("scheme_name", pygame.Rect(x, y, w, 18), None))
        y += 24
        for knob in KNOBS:
            items.append(("knob", pygame.Rect(x, y, w, 34), knob))
            y += 38
        for key, label in TOGGLES:
            items.append(("toggle", pygame.Rect(x, y, w, 22), (key, label)))
            y += 26
        y += 6
        items.append(("label", pygame.Rect(x, y, w, 18), "Presets (click to load)"))
        y += 22
        cw = (w - 6) // 2
        for i, (name, path) in enumerate(self.app.presets):
            items.append(("preset", pygame.Rect(x + (i % 2) * (cw + 6), y + (i // 2) * 28, cw, 24), (name, path)))
        y += ((len(self.app.presets) + 1) // 2) * 28 + 4
        items.append(("field", pygame.Rect(x, y, w - 76, 26), None))
        items.append(("save", pygame.Rect(x + w - 70, y, 70, 26), None))
        y += 32
        items.append(("status", pygame.Rect(x, y, w, 18), None))
        y += 22
        self.items = items
        self.rect = pygame.Rect(0, 0, WIDTH, y)

    def contains(self, pos):
        return self.visible and self.rect.collidepoint(pos)

    # ---- input
    def handle(self, e):
        """Return True when the event belongs to the panel."""
        if self.focus and e.type == pygame.TEXTINPUT:
            self.name = (self.name + e.text)[:32]
            return True
        if self.focus and e.type == pygame.KEYDOWN:
            if e.key in (pygame.K_RETURN, pygame.K_KP_ENTER):
                self.save()
            elif e.key == pygame.K_ESCAPE:
                self.set_focus(False)
            elif e.key == pygame.K_BACKSPACE:
                self.name = self.name[:-1]
            return True
        if e.type == pygame.MOUSEBUTTONDOWN and e.button == 1:
            if not self.rect.collidepoint(e.pos):
                self.set_focus(False)
                return False
            self.click(e.pos)
            return True
        if e.type == pygame.MOUSEBUTTONDOWN:
            return self.rect.collidepoint(e.pos)
        if e.type == pygame.MOUSEMOTION and self.dragging:
            self.drag_to(e.pos[0])
            return True
        if e.type == pygame.MOUSEBUTTONUP and self.dragging:
            self.dragging = None
            return True
        if e.type == pygame.MOUSEWHEEL and self.rect.collidepoint(pygame.mouse.get_pos()):
            for kind, rect, payload in self.items:
                if kind == "knob" and rect.collidepoint(pygame.mouse.get_pos()):
                    key, step = payload[0], payload[4]
                    self.set_value(key, getattr(self.app.settings, key) + step * e.y)
            return True
        return False

    def set_focus(self, on):
        if on and not self.focus:
            pygame.key.start_text_input()
        elif not on and self.focus:
            pygame.key.stop_text_input()
        self.focus = on

    def click(self, pos):
        self.set_focus(False)
        for kind, rect, payload in self.items:
            if not rect.collidepoint(pos):
                continue
            if kind == "scheme":
                self.app.set_scheme(payload)
            elif kind == "knob":
                self.dragging = payload[0]
                self.drag_to(pos[0])
            elif kind == "toggle":
                key = payload[0]
                setattr(self.app.settings, key, not getattr(self.app.settings, key))
                self.app.settings_changed()
            elif kind == "preset":
                self.app.load_preset(payload[1])
                self.name = payload[0]
            elif kind == "field":
                self.set_focus(True)
            elif kind == "save":
                self.save()
            return

    def track(self, rect):
        return pygame.Rect(rect.x + 6, rect.y + 22, rect.w - 12, 6)

    def drag_to(self, x):
        for kind, rect, payload in self.items:
            if kind == "knob" and payload[0] == self.dragging:
                key, _, lo, hi, step, _ = payload
                t = self.track(rect)
                frac = max(0.0, min(1.0, (x - t.x) / t.w))
                self.set_value(key, round((lo + frac * (hi - lo)) / step) * step)

    def set_value(self, key, value):
        lo, hi = LIMITS[key]
        setattr(self.app.settings, key, max(lo, min(hi, value)))
        self.app.settings_changed()

    def save(self):
        name = self.name.strip() or self.app.preset_name
        try:
            path = self.app.save_preset(name)
            self.status = f"Saved presets/{os.path.basename(path)}"
        except (OSError, ValueError) as err:
            self.status = f"Not saved: {err}"
        self.set_focus(False)
        self.layout()

    # ---- drawing
    def text(self, surf, font, s, pos, color=WHITE, anchor="topleft"):
        img = font.render(s, True, color)
        r = img.get_rect(**{anchor: pos})
        surf.blit(img, r)
        return r

    def button(self, surf, rect, label, on, hover, font=None):
        pygame.draw.rect(surf, YELLOW if on else BLACK, rect, border_radius=4)
        pygame.draw.rect(surf, SKY if hover and not on else WHITE, rect, 2 if hover else 1, border_radius=4)
        self.text(surf, font or self.font, label, rect.center, BLACK if on else WHITE, "center")

    def draw(self, surf):
        if not self.visible:
            return
        self.layout()
        s, app = self.app.settings, self.app
        mouse = pygame.mouse.get_pos()
        pygame.draw.rect(surf, BLACK, self.rect)
        pygame.draw.line(surf, WHITE, self.rect.topright, self.rect.bottomright)
        pygame.draw.line(surf, WHITE, self.rect.bottomleft, self.rect.bottomright)
        for kind, rect, payload in self.items:
            hover = rect.collidepoint(mouse)
            if kind == "title":
                self.text(surf, self.title, "FLIGHT LAB", rect.topleft)
                self.text(surf, self.small, "Tab hides this panel", rect.topright, anchor="topright")
            elif kind == "scheme":
                self.button(surf, rect, f"{payload} {SHORT[payload]}", payload == app.pilot.scheme, hover, self.small)
            elif kind == "scheme_name":
                self.text(surf, self.small, f"Scheme {app.pilot.scheme}: {SCHEMES[app.pilot.scheme]}", rect.topleft)
            elif kind == "knob":
                key, label, lo, hi, _, fmt = payload
                val = getattr(s, key)
                self.text(surf, self.font, label, rect.topleft)
                self.text(surf, self.font, fmt.format(val), rect.topright, anchor="topright")
                t = self.track(rect)
                frac = (val - lo) / (hi - lo)
                pygame.draw.rect(surf, WHITE, t, 1)
                pygame.draw.rect(surf, SKY, (t.x, t.y, max(1, int(t.w * frac)), t.h))
                hx = t.x + t.w * frac
                r = 8 if (hover or self.dragging == key) else 6
                pygame.draw.circle(surf, ORANGE, (hx, t.centery), r)
                pygame.draw.circle(surf, WHITE, (hx, t.centery), r, 1)
            elif kind == "toggle":
                key, label = payload
                box = pygame.Rect(rect.x, rect.y + 2, 18, 18)
                pygame.draw.rect(surf, YELLOW if getattr(s, key) else BLACK, box)
                pygame.draw.rect(surf, SKY if hover else WHITE, box, 2 if hover else 1)
                self.text(surf, self.font, f"{label}: {'on' if getattr(s, key) else 'off'}", (rect.x + 26, rect.y + 3))
            elif kind == "label":
                self.text(surf, self.small, payload, rect.topleft)
            elif kind == "preset":
                on = payload[0] == app.preset_name
                label = payload[0] + ("*" if on and app.preset_dirty else "")
                self.button(surf, rect, label, on, hover)
            elif kind == "field":
                pygame.draw.rect(surf, BLACK, rect)
                pygame.draw.rect(surf, SKY if self.focus or hover else WHITE, rect, 2 if self.focus else 1)
                shown = self.name if (self.name or self.focus) else "name to save as"
                caret = "|" if self.focus and (pygame.time.get_ticks() // 500) % 2 else ""
                self.text(surf, self.font, shown + caret, (rect.x + 6, rect.y + 6))
            elif kind == "save":
                self.button(surf, rect, "Save", False, hover)
            elif kind == "status" and self.status:
                self.text(surf, self.small, self.status, rect.topleft)
