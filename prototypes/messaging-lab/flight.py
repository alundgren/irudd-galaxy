"""Flight model and the tank control scheme, locked to one preset. No drawing in here.

Copied from the flight lab (tank scheme with reverse thrust and a time-to-stop auto-brake),
trimmed to the one setup this lab uses.
"""
import math
from dataclasses import dataclass

from pygame.math import Vector2 as V


@dataclass(frozen=True)
class Settings:
    max_speed: float = 300.0  # px/s
    accel: float = 450.0  # px/s^2
    inertia: float = 0.40  # 0 = stops instantly, 1 = fully Newtonian
    turn_rate: float = 200.0  # deg/s
    brake: float = 350.0  # px/s^2, Space
    auto_brake: bool = True
    auto_brake_time: float = 1.0  # seconds to a full stop after letting go, at any speed
    boost: float = 2.0  # multiplies max speed and acceleration while Shift is held
    zoom: float = 0.80
    lead: float = 0.0  # seconds of velocity the camera looks ahead


SETTINGS = Settings()


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
        self.auto_stop_v = None  # velocity captured when an auto-brake stop begins
        self.auto_stop_left = 0.0
        self.auto_stop_T = None

    @property
    def fwd(self):
        return V(math.cos(self.angle), math.sin(self.angle))

    def hold_still(self):
        self.vel = V()
        self.thrusting = self.retro = False
        self.auto_stop_v = None


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
        ship.auto_stop_v = None
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
        # While auto-brake is on, speed falls in a straight line to zero in the
        # chosen number of seconds, from whatever speed the release started at.
        if s.auto_brake and not brake:
            if ship.auto_stop_v is None or ship.auto_stop_T != s.auto_brake_time:
                ship.auto_stop_v = V(v)
                ship.auto_stop_T = s.auto_brake_time
                ship.auto_stop_left = s.auto_brake_time
            ship.auto_stop_left -= dt
            if ship.auto_stop_T <= 0 or ship.auto_stop_left <= 0:
                v = V()
            else:
                v = ship.auto_stop_v * (ship.auto_stop_left / ship.auto_stop_T)
        else:
            ship.auto_stop_v = None
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
    mine: bool = False


def steer(ship, s, c, dt):
    """Tank scheme: A/D rotate, W thrusts forward, S thrusts backward.

    Turns the ship and returns (thrust dir or None, target speed, brake)."""
    turn = math.radians(s.turn_rate) * dt
    vmax = s.max_speed * (s.boost if c.boost else 1.0)
    ship.angle += (c.right - c.left) * turn
    along = (1 if c.up else 0) - (1 if c.down else 0)
    if along == 0:
        return None, 0.0, c.brake
    return (ship.fwd if along > 0 else -ship.fwd), vmax, c.brake
