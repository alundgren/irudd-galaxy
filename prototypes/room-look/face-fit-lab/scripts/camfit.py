# Pinhole camera maths matching a Blender camera with sensor_fit VERTICAL, and a small
# robust Levenberg-Marquardt fit. Pure numpy, so it runs inside Blender.
import numpy as np

W, H = 568, 900          # comparison crop size (render size for the concept camera)
SENSOR_H = 36.0          # mm, sensor_fit VERTICAL


def rotvec_to_mat(r):
    t = np.linalg.norm(r)
    if t < 1e-12:
        return np.eye(3)
    k = r / t
    K = np.array([[0, -k[2], k[1]], [k[2], 0, -k[0]], [-k[1], k[0], 0]])
    return np.eye(3) + np.sin(t) * K + (1 - np.cos(t)) * K @ K


def mat_to_rotvec(R):
    c = np.clip((np.trace(R) - 1) / 2, -1, 1)
    t = np.arccos(c)
    if t < 1e-9:
        return np.zeros(3)
    v = np.array([R[2, 1] - R[1, 2], R[0, 2] - R[2, 0], R[1, 0] - R[0, 1]]) / (2 * np.sin(t))
    return v * t


def project(P, R, C, lens, w=W, h=H):
    """P (n,3) world -> (n,2) image px (y down) and camera-space depth (m, positive).
    R: camera-to-world rotation (Blender matrix_world 3x3), C: camera position."""
    pc = (np.asarray(P) - C) @ R            # world -> camera (columns of R are cam axes)
    depth = -pc[:, 2]
    f = lens / SENSOR_H * h
    x = w / 2 + f * pc[:, 0] / depth
    y = h / 2 - f * pc[:, 1] / depth
    return np.stack([x, y], 1), depth


def _unpack(p):
    return rotvec_to_mat(p[:3]), p[3:6], p[6]


def fit(P, uv, sigma, p0, lens_bounds=(35.0, 200.0), fix_lens=None, iters=200, huber=2.0):
    """Fit rotation vector, position and lens so P projects onto uv. sigma: per-point
    1-sigma (px). Returns params, normalized residuals. Huber-weighted, lens clamped."""
    p = np.array(p0, dtype=float)
    if fix_lens is not None:
        p[6] = fix_lens
    free = [0, 1, 2, 3, 4, 5] + ([] if fix_lens is not None else [6])
    lam = 1e-3

    def res(q):
        R, C, lens = _unpack(q)
        pr, d = project(P, R, C, lens)
        r = ((pr - uv) / sigma[:, None]).ravel()
        if np.any(d <= 0):
            r = r + 1e3
        return r

    def robust(r):
        a = np.abs(r)
        w = np.where(a <= huber, 1.0, huber / np.maximum(a, 1e-9))
        return np.sqrt(w)

    r = res(p)
    for _ in range(iters):
        w = robust(r)
        J = np.zeros((len(r), len(free)))
        for j, k in enumerate(free):
            e = 1e-6 if k < 3 else (1e-6 if k < 6 else 1e-3)
            q = p.copy()
            q[k] += e
            J[:, j] = (res(q) - r) / e
        Jw = J * w[:, None]
        rw = r * w
        A = Jw.T @ Jw
        g = Jw.T @ rw
        while True:
            step = -np.linalg.solve(A + lam * np.diag(np.diag(A) + 1e-12), g)
            q = p.copy()
            q[free] += step
            q[6] = np.clip(q[6], *lens_bounds)
            rn = res(q)
            if np.sum((rn * robust(rn)) ** 2) < np.sum((r * w) ** 2):
                p, r = q, rn
                lam = max(lam / 3, 1e-9)
                break
            lam *= 4
            if lam > 1e8:
                break
        if lam > 1e8 or np.linalg.norm(step) < 1e-10:
            break
    return p, r


def look_at(C, target, roll_deg=0.0):
    """Camera-to-world rotation for a camera at C looking at target, Z up, then rolled."""
    f = np.asarray(target, float) - C
    f /= np.linalg.norm(f)
    z = -f
    x = np.cross(np.array([0, 0, 1.0]), z)
    x /= np.linalg.norm(x)
    y = np.cross(z, x)
    R = np.stack([x, y, z], 1)
    a = np.radians(roll_deg)
    Rr = np.array([[np.cos(a), -np.sin(a), 0], [np.sin(a), np.cos(a), 0], [0, 0, 1]])
    return R @ Rr
