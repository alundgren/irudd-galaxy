# Small signed-distance helpers used to block out Bram's head as one blended surface.
# Every function takes point arrays X, Y, Z (metres) and returns a distance array:
# negative inside, positive outside. smin/smax blend two forms with a soft fillet.
import numpy as np


def length3(x, y, z):
    return np.sqrt(x * x + y * y + z * z)


def sphere(X, Y, Z, c, r):
    return length3(X - c[0], Y - c[1], Z - c[2]) - r


def ellipsoid(X, Y, Z, c, r):
    px, py, pz = (X - c[0]) / r[0], (Y - c[1]) / r[1], (Z - c[2]) / r[2]
    k0 = np.sqrt(px * px + py * py + pz * pz)
    qx, qy, qz = px / r[0], py / r[1], pz / r[2]
    k1 = np.sqrt(qx * qx + qy * qy + qz * qz) + 1e-9
    return k0 * (k0 - 1.0) / k1


def capsule(X, Y, Z, a, b, ra, rb=None):
    """Round cone from a (radius ra) to b (radius rb)."""
    rb = ra if rb is None else rb
    ax, ay, az = X - a[0], Y - a[1], Z - a[2]
    bx, by, bz = b[0] - a[0], b[1] - a[1], b[2] - a[2]
    bb = bx * bx + by * by + bz * bz
    h = np.clip((ax * bx + ay * by + az * bz) / bb, 0.0, 1.0)
    return length3(ax - bx * h, ay - by * h, az - bz * h) - (ra + (rb - ra) * h)


def chain(X, Y, Z, pts, radii):
    """Capsules through a list of points with per-point radius."""
    d = None
    for i in range(len(pts) - 1):
        di = capsule(X, Y, Z, pts[i], pts[i + 1], radii[i], radii[i + 1])
        d = di if d is None else np.minimum(d, di)
    return d


def smin(a, b, k):
    if k <= 0:
        return np.minimum(a, b)
    h = np.clip(0.5 + 0.5 * (b - a) / k, 0.0, 1.0)
    return b + (a - b) * h - k * h * (1.0 - h)


def smax(a, b, k):
    return -smin(-a, -b, k)


def subtract(a, cut, k):
    """a with cut removed, fillet k."""
    return smax(a, -cut, k)


def local(X, Y, Z, origin, ax_s, ax_t, ax_w):
    """Coordinates of the points in an orthonormal frame (s, t, w) at origin."""
    x, y, z = X - origin[0], Y - origin[1], Z - origin[2]
    s = x * ax_s[0] + y * ax_s[1] + z * ax_s[2]
    t = x * ax_t[0] + y * ax_t[1] + z * ax_t[2]
    w = x * ax_w[0] + y * ax_w[1] + z * ax_w[2]
    return s, t, w
