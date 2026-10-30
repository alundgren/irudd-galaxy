"""Flight model, control schemes and presets. No drawing in here."""
import json
import math
import os
import re
from dataclasses import asdict, dataclass, fields

from pygame.math import Vector2 as V


@dataclass
class Settings:
    max_speed: float = 400.0  # px/s, about 20% faster than Proto 1 "Normal" (330)
    accel: float = 1320.0  # px/s^2
    inertia: float = 0.3  # 0 = stops instantly, 1 = fully Newtonian
    turn_rate: float = 300.0  # deg/s
    brake: float = 1600.0  # px/s^2, Space (and S in the schemes that brake)
    auto_brake: bool = False
    auto_brake_strength: float = 100.0  # px/s^2 of the whole release stop while auto-brake is on. 0 coasts.
    boost: float = 1.6  # multiplies max speed and acceleration while Shift is held
    zoom: float = 1.0
    lead: float = 0.25  # seconds of velocity the camera looks ahead
    course_hold: bool = True  # WASD: tap a side key to nudge the course and keep it

    def copy(self):
        return Settings(**asdict(self))


# key, label, low, high, wheel step, value format
KNOBS = [
    ("max_speed", "Max speed", 100, 1200, 10, "{:.0f} px/s"),
    ("accel", "Acceleration", 100, 5000, 20, "{:.0f} px/s²"),
    ("inertia", "Inertia (0 stop, 1 Newton)", 0.0, 1.0, 0.01, "{:.2f}"),
    ("turn_rate", "Turn rate", 30, 900, 10, "{:.0f} °/s"),
    ("brake", "Brake strength", 0, 6000, 50, "{:.0f} px/s²"),
    ("auto_brake_strength", "Auto-brake (0 coasts)", 0, 6000, 50, "{:.0f} px/s²"),
    ("boost", "Boost (Shift)", 1.0, 3.0, 0.05, "x{:.2f}"),
    ("zoom", "Camera zoom", 0.2, 2.0, 0.05, "{:.2f}"),
    ("lead", "Camera lead", 0.0, 1.0, 0.05, "{:.2f} s"),
]
TOGGLES = [("auto_brake", "Auto-brake on release"), ("course_hold", "WASD course hold (tap to nudge)")]
LIMITS = {k: (lo, hi) for k, _, lo, hi, _, _ in KNOBS}

SCHEMES = {
    1: "WASD screen (analog)",
    2: "Mouse-aim",
    3: "Tank / Asteroids",
    4: "Click to fly",
    5: "Hybrid: aim + strafe",
}
SHORT = {1: "WASD", 2: "Aim", 3: "Tank", 4: "Click", 5: "Hybrid"}
HINTS = {
    1: "WASD/arrows fly in screen directions, tap a side key to nudge the course, Shift boost, Space brake",
    2: "Ship faces the cursor, W or right mouse thrusts, S/Space brakes, Shift boost",
    3: "A/D rotate, W thrusts forward, S thrusts backward, Shift boost, Space brakes",
    4: "Click or hold left mouse to fly there, right click/Space stops, WASD takes over, Shift boost",
    5: "Ship faces the cursor, W/S forward/back, A/D strafe, right mouse thrusts, Space brakes",
}


def wrap_angle(a):
    return (a + math.pi) % math.tau - math.pi


def rotate_toward(angle, target, step):
    """Turn angle toward target by at most step. Returns (new angle, error left)."""
    err = wrap_angle(target - angle)
    move = max(-step, min(step, err))
    return angle + move, err - move


def coast_tau(inertia):
    """Seconds for leftover velocity to fall to 1/e: 0 at inertia 0, never at inertia 1."""
    if inertia <= 0:
        return 0.0
    if inertia >= 1:
        return math.inf
    return 0.35 * (inertia / (1 - inertia)) ** 1.3


def keep_factor(dt, inertia):
    tau = coast_tau(inertia)
    if tau == 0:
        return 0.0
    if tau == math.inf:
        return 1.0
    return math.exp(-dt / tau)


class Ship:
    def __init__(self, pos=(0, 0)):
        self.pos = V(pos)
        self.vel = V()
        self.angle = -math.pi / 2
        self.thrusting = False
        self.retro = False

    @property
    def fwd(self):
        return V(math.cos(self.angle), math.sin(self.angle))


def fly(ship, s, thrust, target_speed, brake, boosting, dt):
    """Advance the ship one step.

    thrust: unit direction the engines push, or None for no input.
    target_speed: speed wanted along thrust (capped at max speed).
    Inertia sets how much of the old velocity survives: sideways motion swings
    round into the new direction (grip), and coasting bleeds off.
    """
    mult = s.boost if boosting else 1.0
    vmax, acc = s.max_speed * mult, s.accel * mult
    keep = keep_factor(dt, s.inertia)
    v = V(ship.vel)
    ship.thrusting = thrust is not None and target_speed > 0
    ship.retro = bool(ship.thrusting and thrust.dot(ship.fwd) < 0)
    rate = s.brake if brake else 0.0
    if ship.thrusting:
        along = v.dot(thrust)
        side = v - thrust * along
        if along >= 0:
            # speed-preserving grip: lost sideways speed turns into forward speed
            along = math.sqrt(along * along + side.length_squared() * (1 - keep * keep))
        else:
            along *= keep
        target = min(target_speed, vmax)
        if along < target:
            along = min(target, along + acc * dt)
        else:
            along = max(target, min(along - acc * dt, target + (along - target) * keep))
        v = thrust * along + side * keep
    else:
        # Auto-brake replaces the inertia coast. Otherwise a low slider still
        # stops dead, because inertia 0.4 bleeds speed in a fraction of a second.
        if s.auto_brake and not brake:
            rate = s.auto_brake_strength
        else:
            v *= keep
    if rate > 0:
        sp = v.length()
        if sp > 0:
            v *= max(0.0, sp - rate * dt) / sp
    cap = s.max_speed * max(1.0, s.boost)
    if v.length() > cap:
        v.scale_to_length(cap)
    ship.vel = v
    ship.pos += v * dt


@dataclass
class Controls:
    up: bool = False
    down: bool = False
    left: bool = False
    right: bool = False
    boost: bool = False
    brake: bool = False
    mouse: V = None  # cursor in world space, None when off-window or over the panel
    lmb: bool = False
    rmb: bool = False
    rmb_click: bool = False


class Pilot:
    """Turns raw controls into a thrust request, according to the active scheme."""

    def __init__(self):
        self.scheme = 1
        self.target = None  # click-to-fly destination
        self.prev_keys = frozenset()
        self.holding = False
        self.settled = True

    def set_scheme(self, n):
        self.scheme = n
        self.target = None
        self.holding = False
        self.prev_keys = frozenset()

    def steer(self, ship, s, c, dt):
        """Turn the ship and return (thrust dir or None, target speed, brake)."""
        turn = math.radians(s.turn_rate) * dt
        vmax = s.max_speed * (s.boost if c.boost else 1.0)
        n = self.scheme
        if n == 1:
            return self._wasd(ship, c, turn, vmax, s.course_hold)
        if n == 3:
            ship.angle += (c.right - c.left) * turn
            along = (1 if c.up else 0) - (1 if c.down else 0)
            if along == 0:
                return None, 0.0, c.brake
            return (ship.fwd if along > 0 else -ship.fwd), vmax, c.brake
        if n == 4:
            return self._click(ship, s, c, turn, vmax)
        if c.mouse is not None and (c.mouse - ship.pos).length_squared() > 144:
            to = c.mouse - ship.pos
            ship.angle, _ = rotate_toward(ship.angle, math.atan2(to.y, to.x), turn)
        fwd = ship.fwd
        if n == 2:
            return (fwd if (c.up or c.rmb) else None), vmax, c.down or c.brake
        # 5: hybrid, ship-relative strafing
        right = V(-fwd.y, fwd.x)
        push = fwd * ((c.up or c.rmb) - c.down) + right * (c.right - c.left)
        return (push.normalize() if push.length_squared() > 0 else None), vmax, c.brake

    def _wasd(self, ship, c, turn, vmax, hold):
        k = V(c.right - c.left, c.down - c.up)
        keys = frozenset(n for n in ("up", "down", "left", "right") if getattr(c, n))
        if keys != self.prev_keys:
            # releasing a side key before the turn finished keeps the in-between course
            nudged = bool(keys) and keys < self.prev_keys and not self.settled
            self.holding = nudged and k.length_squared() > 0 and ship.fwd.dot(k.normalize()) > 0
            self.prev_keys = keys
        if not hold:
            self.holding = False
        if k.length_squared() == 0:
            return None, 0.0, c.brake
        if not self.holding:
            want = math.atan2(k.y, k.x)
            if abs(wrap_angle(want - ship.angle)) > math.radians(135):
                ship.angle = want  # reversing: flip now rather than arc the long way
            ship.angle, err = rotate_toward(ship.angle, want, turn)
            self.settled = abs(err) < 0.035
        return ship.fwd, vmax, c.brake

    def _click(self, ship, s, c, turn, vmax):
        if c.up or c.down or c.left or c.right:
            self.target = None
            return self._wasd(ship, c, turn, vmax, False)
        if c.rmb_click or c.brake:
            self.target = None
        if c.lmb and c.mouse is not None:
            self.target = V(c.mouse)
        if self.target is None:
            return None, 0.0, c.brake
        to = self.target - ship.pos
        dist = to.length()
        if dist < 6:
            if ship.vel.length() < 40:
                self.target = None
            return None, 0.0, True
        to_dir = to / dist
        arrive = min(vmax, math.sqrt(1.6 * s.accel * dist))
        want = to_dir * arrive - ship.vel * s.inertia  # cancel drift when there is lots of it
        d = want.normalize() if want.length() > 1 else to_dir
        ship.angle, err = rotate_toward(ship.angle, math.atan2(d.y, d.x), turn)
        return ship.fwd, arrive * (0.2 + 0.8 * max(0.0, math.cos(err))), False


# ---------------------------------------------------------------- presets
def slug(name):
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")[:40]


def list_presets(folder):
    """[(display name, path)] for every JSON preset in folder, sorted by name."""
    out = []
    if os.path.isdir(folder):
        for fn in os.listdir(folder):
            if fn.endswith(".json"):
                path = os.path.join(folder, fn)
                try:
                    with open(path, encoding="utf-8") as f:
                        name = json.load(f).get("name") or fn[:-5]
                except (OSError, ValueError, AttributeError):
                    continue
                out.append((str(name), path))
    return sorted(out, key=lambda p: p[0].lower())


def load_preset(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    s = Settings()
    values = data.get("settings", {})
    for fld in fields(Settings):
        if fld.name not in values:
            continue
        val = values[fld.name]
        if fld.type is bool or fld.type == "bool":
            setattr(s, fld.name, bool(val))
        else:
            lo, hi = LIMITS[fld.name]
            setattr(s, fld.name, max(lo, min(hi, float(val))))
    return str(data.get("name") or os.path.basename(path)[:-5]), s


def save_preset(folder, name, settings):
    name = name.strip()
    if not slug(name):
        raise ValueError("preset name needs a letter or digit")
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, slug(name) + ".json")
    values = {k: (round(v, 3) if isinstance(v, float) else v) for k, v in asdict(settings).items()}
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"name": name, "settings": values}, f, indent=2)
        f.write("\n")
    return path
