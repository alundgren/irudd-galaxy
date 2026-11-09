# Bram's head, neck and ears as one blended form. Coordinates in metres: the origin is
# the midpoint between the eyes' centres, -Y is the way he faces, +Z up, +X his left.
# Each block below is one anatomical mass; edit the numbers and rebuild.
import numpy as np

from sdf_lib import capsule, chain, ellipsoid, local, smax, smin, sphere, subtract

EYE_R = 0.0124
EYE_X = 0.0322
EYE_Y = -0.0725
EYE_Z = 0.0
FACE_W = 1.05   # the whole head is modelled narrow and widened by this factor


# Expression of the lids per side (+1 is his left, the side of the half smile):
# upper lid opening, lower lid opening, and how much the cheek pushes the lower lid up.
LIDS = {-1: (0.345, -0.305, 0.0), 1: (0.335, -0.300, 0.100)}


def lid_v(s, which, side):
    """Lid edge angle (radians above the eye's centre) at s in [-1, 1], inner to outer corner."""
    top_open, bot_open, push = LIDS[side]
    sc = np.clip(s, -1, 1)
    tilt = 0.055 * sc
    if which == "top":
        return top_open * np.power(1 - sc * sc, 0.62) * (1 - 0.14 * sc) + tilt
    lift = push * np.power(1 - sc * sc, 0.6) * (0.7 + 0.3 * sc)
    return bot_open * np.power(1 - sc * sc, 0.85) * (1 + 0.16 * sc) + tilt + lift


def eye_aperture(X, Y, Z, side):
    """Signed distance of the opening between the lids (negative inside the opening)."""
    cx = side * EYE_X
    dx, dy, dz = X - cx, Y - EYE_Y, Z - EYE_Z
    dist = np.sqrt(dx * dx + dy * dy + dz * dz) + 1e-9
    lat = dx * side  # positive towards the outer corner
    u = np.arctan2(lat, -dy)
    v = np.arcsin(np.clip(dz / dist, -1, 1))
    u_lat, u_med = 1.12, 1.06
    s = np.where(u > 0, u / u_lat, u / u_med)
    top = lid_v(s, "top", side)
    bot = lid_v(s, "bot", side)
    g = np.maximum(np.where(u > 0, u - u_lat, -u - u_med), np.maximum(v - top, bot - v))
    return np.maximum(g * dist, dist - (EYE_R + 0.010))


def lid_curve(side, which, offset_v, radius_r, n=13):
    """Points along the upper or lower lid margin, pushed by offset_v radians."""
    pts = []
    for i in range(n):
        s = (-1 + 2 * i / (n - 1)) * 0.93
        u = s * (1.12 if s > 0 else 1.06)
        v = float(lid_v(np.array(s), which, side)) + offset_v
        r = EYE_R + radius_r
        x = side * EYE_X + side * r * np.cos(v) * np.sin(u)
        y = EYE_Y - r * np.cos(v) * np.cos(u)
        z = EYE_Z + r * np.sin(v)
        pts.append((x, y, z))
    return pts


def smile_warp(X, Y, Z):
    """Coordinates for the lower-face features, moved by his half smile on his left (+X).
    Sampling a feature at the warped point moves it up, out and back near that corner."""
    front = np.clip((-Y - 0.055) / 0.02, 0.0, 1.0)
    wm = np.exp(-(((X - 0.028) / 0.018) ** 2 + ((Z + 0.073) / 0.017) ** 2)) * front
    wc = np.exp(-(((X - 0.036) / 0.020) ** 2 + ((Z + 0.044) / 0.018) ** 2)) * front
    return X - 0.0030 * wm, Y - 0.0022 * wm, Z - 0.0062 * wm - 0.0036 * wc


def ear(X, Y, Z, side):
    origin = (side * 0.0735, 0.0135, -0.016)
    flare, tilt = np.radians(13), np.radians(14)
    # w: outward normal of the ear, t: up along the ear, s: towards the back of the ear
    w = np.array([side * np.cos(flare), np.sin(flare), 0.0])
    t = np.array([0.0, np.sin(tilt), np.cos(tilt)])
    t = t - w * np.dot(t, w)
    t /= np.linalg.norm(t)
    s = np.cross(t, w) * side
    if np.dot(s, (0, 1, 0)) < 0:
        s = -s
    S, T, W = local(X, Y, Z, origin, s, t, w)
    zero = np.zeros_like(S)

    def at(ss, tt, ww):
        return (ss, tt, ww)

    body = ellipsoid(S, T, W, (0.002, 0.002, 0.0035), (0.0155, 0.0305, 0.0055))
    helix = chain(S, T, W,
                  [at(-0.009, 0.004, 0.006), at(-0.007, 0.019, 0.0085), at(0.0, 0.0295, 0.009),
                   at(0.0105, 0.025, 0.0085), at(0.0155, 0.009, 0.008), at(0.0135, -0.010, 0.0072),
                   at(0.008, -0.020, 0.006)],
                  [0.0022, 0.0030, 0.0034, 0.0034, 0.0032, 0.0030, 0.0028])
    lobe = ellipsoid(S, T, W, (0.0035, -0.0235, 0.0045), (0.0078, 0.0085, 0.0045))
    ear_d = smin(smin(body, helix, 0.003), lobe, 0.004)
    concha = ellipsoid(S, T, W, (-0.0015, -0.0045, 0.0095), (0.0085, 0.0105, 0.0068))
    scapha = chain(S, T, W, [at(-0.002, 0.021, 0.0105), at(0.007, 0.020, 0.0105),
                             at(0.0115, 0.006, 0.0102), at(0.009, -0.011, 0.0095)],
                   [0.0026, 0.0028, 0.0026, 0.0022])
    ear_d = subtract(ear_d, concha, 0.0025)
    ear_d = subtract(ear_d, scapha, 0.0015)
    antihelix = chain(S, T, W, [at(0.0055, -0.013, 0.0072), at(0.0075, 0.000, 0.0082),
                                at(0.004, 0.014, 0.0082), at(-0.003, 0.0175, 0.0075)],
                      [0.0024, 0.0027, 0.0022, 0.0018])
    ear_d = smin(ear_d, antihelix, 0.002)
    tragus = ellipsoid(S, T, W, (-0.0105, -0.0055, 0.006), (0.0032, 0.0045, 0.0032))
    ear_d = smin(ear_d, tragus, 0.002)
    canal = sphere(S, T, W, (-0.0055, -0.004, 0.003), 0.0032)
    ear_d = subtract(ear_d, canal, 0.002)
    del zero
    return ear_d


def head(X, Y, Z):
    X = X / FACE_W
    # Skull and forehead
    d = ellipsoid(X, Y, Z, (0.0, 0.014, 0.026), (0.0735, 0.0975, 0.091))
    d = smin(d, ellipsoid(X, Y, Z, (0.0, -0.040, 0.040), (0.062, 0.054, 0.060)), 0.02)
    # Temples are a little hollow behind the brow
    for sx in (-1, 1):
        d = subtract(d, ellipsoid(X, Y, Z, (sx * 0.080, -0.040, 0.020), (0.010, 0.020, 0.018)), 0.014)

    # Mid-face (maxilla) and the muzzle that carries the lips
    d = smin(d, ellipsoid(X, Y, Z, (0.0, -0.034, -0.030), (0.0630, 0.064, 0.070)), 0.022)
    d = smin(d, ellipsoid(X, Y, Z, (0.0, -0.080, -0.068), (0.041, 0.026, 0.031)), 0.016)

    # Lower face and jaw: one volume trimmed by planes, so the jawline is a clear edge
    low = ellipsoid(X, Y, Z, (0.0, -0.030, -0.070), (0.064, 0.072, 0.064))
    ay = np.minimum(Y, 0.0)
    half_w = 0.0660 + 0.36 * ay + 0.10 * np.clip(Z + 0.075, -0.05, 0.05)
    low = smax(low, np.abs(X) - half_w, 0.012)
    gy, gz, my, mz = 0.004, -0.083, -0.080, -0.123
    ny, nz = (gz - mz), -(gy - my)
    nn = np.hypot(ny, nz)
    under = ((Y - gy) * ny + (Z - gz) * nz) / nn
    low = smax(low, -under if nz > 0 else under, 0.008)
    d = smin(d, low, 0.018)
    # Chin: broad, squared front
    d = smin(d, ellipsoid(X, Y, Z, (0.0, -0.0885, -0.108), (0.0215, 0.0135, 0.0165)), 0.012)

    # Brow ridge: heavy, with an asymmetric arch (higher on his right, -X)
    for sx in (-1, 1):
        arch = 0.0030 if sx < 0 else -0.0010
        brow = chain(X, Y, Z, [(sx * 0.006, -0.0865, 0.0150), (sx * 0.022, -0.0880, 0.0200 + arch * 0.6),
                               (sx * 0.040, -0.0805, 0.0205 + arch), (sx * 0.056, -0.0665, 0.0140 + arch * 0.3)],
                     [0.0070, 0.0080, 0.0072, 0.0058])
        d = smin(d, brow, 0.016)

    # Broad cheek planes from the cheekbone forward; only a faint hollow far back
    for sx in (-1, 1):
        d = smin(d, ellipsoid(X, Y, Z, (sx * 0.0470, -0.0620, -0.0215), (0.0185, 0.0170, 0.0128)), 0.022)
        d = subtract(d, ellipsoid(X, Y, Z, (sx * 0.0700, -0.040, -0.062), (0.008, 0.018, 0.015)), 0.026)

    # Eye sockets, the lids wrapped round each eyeball, and the opening between them
    for sx in (-1, 1):
        d = subtract(d, ellipsoid(X, Y, Z, (sx * 0.033, -0.0855, 0.002), (0.019, 0.011, 0.012)), 0.010)
        d = smin(d, sphere(X, Y, Z, (sx * EYE_X, EYE_Y, EYE_Z), EYE_R + 0.0024), 0.007)
        # Hooded outer upper lid (brow fat) and the lid crease beneath it
        d = smin(d, ellipsoid(X, Y, Z, (sx * 0.0385, -0.0835, 0.0115), (0.0115, 0.0050, 0.0036)), 0.006)
        d = subtract(d, chain(X, Y, Z, lid_curve(sx, "top", 0.25, 0.0030, 11), [0.0009] * 11), 0.0012)
        # Lower lid: soft bag with a shallow groove under it
        d = smin(d, chain(X, Y, Z, lid_curve(sx, "bot", -0.12, 0.0022, 9), [0.0014] * 9), 0.0035)
        d = subtract(d, chain(X, Y, Z, lid_curve(sx, "bot", -0.46, 0.0036, 9), [0.0007] * 9), 0.0060)
        d = subtract(d, eye_aperture(X, Y, Z, sx), 0.0006)
        d = subtract(d, sphere(X, Y, Z, (sx * EYE_X, EYE_Y, EYE_Z), EYE_R + 0.0004), 0.0004)

    # Nose: a short broad bridge, a rounded tip that does not hang, and wide wings joined
    # to the tip by one base, so the nose reads as one form rather than three bulbs
    nose = capsule(X, Y, Z, (0.0, -0.0865, 0.002), (0.0, -0.1110, -0.031), 0.0074, 0.0098)
    nose = smin(nose, ellipsoid(X, Y, Z, (0.0, -0.1112, -0.0385), (0.0128, 0.0105, 0.0098)), 0.006)
    for sx in (-1, 1):
        nose = smin(nose, capsule(X, Y, Z, (sx * 0.0068, -0.0915, -0.004), (sx * 0.0125, -0.1015, -0.032),
                                  0.0062, 0.0070), 0.005)
    nose = smin(nose, ellipsoid(X, Y, Z, (0.0, -0.1012, -0.0445), (0.0228, 0.0105, 0.0086)), 0.006)
    d = smin(d, nose, 0.008)
    d = subtract(d, ellipsoid(X, Y, Z, (0.0, -0.0975, 0.0030), (0.0090, 0.0040, 0.0060)), 0.007)
    for sx in (-1, 1):
        ala = ellipsoid(X, Y, Z, (sx * 0.0172, -0.0992, -0.0455), (0.0094, 0.0096, 0.0078))
        d = smin(d, ala, 0.0055)
        crease = chain(X, Y, Z, [(sx * 0.0222, -0.0975, -0.0365), (sx * 0.0274, -0.0932, -0.0440),
                                 (sx * 0.0264, -0.0942, -0.0520), (sx * 0.0192, -0.0998, -0.0540)],
                       [0.0008, 0.0009, 0.0009, 0.0007])
        d = subtract(d, crease, 0.0030)
    d = smin(d, capsule(X, Y, Z, (0.0, -0.1118, -0.0440), (0.0, -0.1030, -0.0515), 0.0040, 0.0035), 0.003)
    for sx in (-1, 1):
        nostril = ellipsoid(X, Y, Z, (sx * 0.0096, -0.1032, -0.0502), (0.0050, 0.0058, 0.0030))
        d = subtract(d, nostril, 0.0014)

    # Lower face features move with the half smile
    WX, WY, WZ = smile_warp(X, Y, Z)
    for sx in (-1, 1):
        # Apple of the cheek
        big = 1.12 if sx > 0 else 0.88
        d = smin(d, ellipsoid(WX, WY, WZ, (sx * 0.0365, -0.0815, -0.0310), (0.0205 * big, 0.0085 * big, 0.0125 * big)), 0.022)
        # Philtrum ridge
        d = smin(d, capsule(WX, WY, WZ, (sx * 0.0040, -0.1020, -0.0530), (sx * 0.0060, -0.1072, -0.0655),
                            0.0013, 0.0017), 0.0035)
    upper = ellipsoid(WX, WY, WZ, (0.0, -0.1008, -0.0700), (0.0270, 0.0112, 0.0078))
    upper = smin(upper, ellipsoid(WX, WY, WZ, (0.0, -0.1055, -0.0688), (0.0100, 0.0062, 0.0054)), 0.003)
    lower = ellipsoid(WX, WY, WZ, (0.0, -0.0995, -0.0826), (0.0250, 0.0128, 0.0100))
    d = smin(d, upper, 0.0045)
    d = smin(d, lower, 0.0040)
    pts = []
    for i in range(15):
        xx = -0.0278 + 0.0556 * i / 14
        xr = xx / 0.0278
        zz = -0.0752 + 0.0022 * xr ** 4 + 0.0022 * max(xr, 0.0) ** 3 - 0.0006 * max(-xr, 0.0) ** 3
        yy = -0.1095 + 0.0125 * xr * xr
        pts.append((xx, yy, zz))
    radii = [0.0013 - 0.0003 * (1 - abs(-1 + 2 * i / 14)) for i in range(15)]
    d = subtract(d, chain(WX, WY, WZ, pts, radii), 0.0010)
    # Smile dimple at his left mouth corner
    d = subtract(d, sphere(WX, WY, WZ, (0.0292, -0.0960, -0.0752), 0.0014), 0.0045)
    d = subtract(d, ellipsoid(WX, WY, WZ, (0.0, -0.1045, -0.0940), (0.0150, 0.0040, 0.0034)), 0.006)

    # Neck, the muscles that frame it and the larynx
    nx = X / 1.20
    neck = capsule(nx, Y, Z, (0.0, 0.014, -0.058), (0.0, 0.028, -0.30), 0.0555, 0.0590)
    d = smin(d, neck, 0.026)
    for sx in (-1, 1):
        scm = capsule(X, Y, Z, (sx * 0.056, 0.022, -0.050), (sx * 0.016, -0.034, -0.205), 0.0115, 0.0105)
        d = smin(d, scm, 0.022)
    d = smin(d, ellipsoid(X, Y, Z, (0.0, -0.034, -0.148), (0.0075, 0.0055, 0.0110)), 0.012)

    for sx in (-1, 1):
        d = smin(d, ear(X, Y, Z, sx), 0.0035)
    return d


BOUNDS = ((-0.112, 0.112), (-0.140, 0.125), (-0.300, 0.130))
