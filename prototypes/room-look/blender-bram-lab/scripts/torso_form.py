# Bram's jacket over the shoulders and upper chest, and his raised collar, as blended
# forms in the same coordinates as head_form.py (origin between the eyes, -Y forward).
import numpy as np

from sdf_lib import capsule, chain, ellipsoid, smax, smin, subtract

FIELD = "jacket"
BOUNDS = ((-0.300, 0.300), (-0.175, 0.185), (-0.540, -0.120))
BOTTOM = -0.52


def _neck_hole(X, Y, Z):
    return capsule(X / 1.13, Y, Z, (0.0, 0.026, -0.10), (0.0, 0.032, -0.32), 0.0625, 0.066)


def jacket(X, Y, Z):
    Z = Z - 0.045
    d = ellipsoid(X, Y, Z, (0.0, 0.018, -0.445), (0.205, 0.125, 0.205))
    for sx in (-1, 1):
        # Trapezius slope from the collar to the shoulder, and the padded shoulder
        d = smin(d, capsule(X, Y, Z, (sx * 0.055, 0.030, -0.205), (sx * 0.185, 0.012, -0.290), 0.044, 0.047), 0.035)
        d = smin(d, ellipsoid(X, Y, Z, (sx * 0.200, 0.008, -0.322), (0.066, 0.072, 0.060)), 0.030)
        # Upper arm in a heavy sleeve
        d = smin(d, capsule(X, Y, Z, (sx * 0.212, 0.010, -0.330), (sx * 0.238, 0.024, -0.560), 0.064, 0.060), 0.025)
        # Chest mass
        d = smin(d, ellipsoid(X, Y, Z, (sx * 0.075, -0.040, -0.390), (0.085, 0.070, 0.090)), 0.040)
        # Fabric folds: a pull from the collar towards the armpit, a sag at the sleeve head
        d = subtract(d, capsule(X, Y, Z, (sx * 0.150, -0.072, -0.330), (sx * 0.172, -0.068, -0.430), 0.004, 0.003), 0.008)
        d = smin(d, capsule(X, Y, Z, (sx * 0.222, -0.048, -0.380), (sx * 0.245, -0.040, -0.470), 0.006, 0.005), 0.012)
        d = subtract(d, capsule(X, Y, Z, (sx * 0.180, 0.100, -0.300), (sx * 0.120, 0.120, -0.400), 0.004, 0.003), 0.010)
    d = subtract(d, _neck_hole(X, Y, Z + 0.045), 0.010)
    # Open front: a V from the collar down to the chest, rounded at its edges
    zz = Z + 0.045
    half = np.clip(0.060 * (zz + 0.430) / 0.200, 0.0, 0.075) + 0.012
    vcut = smax(np.abs(X) - half, Y + 0.020, 0.004)
    vcut = smax(vcut, zz + 0.445, 0.004)
    d = subtract(d, vcut, 0.007)
    return smax(d, BOTTOM - (Z + 0.045), 0.004)


def collar(X, Y, Z):
    """Raised mustard collar: a thick flared shell round the neck, open at the front."""
    cx, cy = 0.0, 0.020
    dx, dy = (X - cx) / 1.08, Y - cy
    rad = np.sqrt(dx * dx + dy * dy)
    theta = np.arctan2(dx, -dy)  # 0 at the front, +pi/2 at his left
    a = np.abs(theta)
    zb = -0.225
    h = np.clip(Z - zb, 0.0, 0.2)
    r_mid = 0.070 + 0.36 * h + 1.4 * h * h
    shell = np.abs(rad - r_mid) - 0.0045
    # Height of the upper edge: high at the back and sides, falling into the front V
    t = np.clip((a - 0.55) / 0.95, 0.0, 1.0)
    top = -0.200 + 0.098 * (t * t * (3 - 2 * t))
    top = top + 0.006 * np.cos(2 * theta) * t
    # Collar points: the front corners rise into wings
    top = top + 0.016 * np.exp(-((a - 0.95) / 0.22) ** 2)
    d = smax(shell, Z - top, 0.004)
    d = smax(d, zb - Z, 0.004)
    # Front opening
    a0 = 0.30 + 5.0 * h
    d = smax(d, a0 - a, 0.006)
    # Upper corners roll outward slightly: thicker rolled edge
    edge = np.abs(Z - top) - 0.006
    rolled = smax(np.abs(rad - (r_mid + 0.0025)) - 0.0062, edge, 0.003)
    rolled = smax(rolled, a0 - a, 0.006)
    rolled = smax(rolled, zb - Z, 0.004)
    return smin(d, rolled, 0.003)


def shirt(X, Y, Z):
    """Cream shirt: the chest inside the jacket's V and a soft standing collar round the neck."""
    d = ellipsoid(X, Y, Z, (0.0, 0.020, -0.405), (0.150, 0.105, 0.200))
    d = smin(d, ellipsoid(X, Y, Z, (0.0, -0.020, -0.330), (0.080, 0.070, 0.080)), 0.03)
    # Placket down the middle
    d = smin(d, capsule(X, Y, Z, (0.0, -0.090, -0.300), (0.0, -0.098, -0.470), 0.004), 0.006)
    d = subtract(d, _neck_hole(X, Y, Z) + 0.004, 0.008)
    cx, cy = 0.0, 0.016
    dx, dy = (X - cx) / 1.13, Y - cy
    rad = np.sqrt(dx * dx + dy * dy)
    theta = np.arctan2(dx, -dy)
    a = np.abs(theta)
    band = np.abs(rad - 0.0640) - 0.0018
    top = -0.150 - 0.026 * np.clip(1 - a / 1.2, 0, 1)
    band = smax(band, Z - top, 0.002)
    band = smax(band, -0.240 - Z, 0.002)
    band = smax(band, 0.22 - a, 0.002)
    return smin(d, band, 0.004)
