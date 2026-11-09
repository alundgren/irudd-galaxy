# Step 5 (arm C). Bounded landmark-driven geometry fit, stored as the shape key
# Fit_Concept on Bram_Head. The Basis (the Bram lab head) is not changed.
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/fit_geometry.py
#
# For each "form" and "expr" landmark (nose, chin, mouth) the concept-to-model error in
# the fitted camera's image is turned into a 3D move:
#   - midline points (nose tip, subnasale, lips, chin) move only within the face's centre
#     plane (y, z), solved through the projection's derivative, so the face stays
#     symmetric. Forward/back moves would shift the points sideways in this three-quarter
#     view, but that is depth the view cannot measure, so they are made expensive (ridge
#     solve) and the sideways error mostly stays as a measured residual;
#   - side points (mouth corners, nose wing) move in the camera's image plane (minimal
#     move; depth along the view ray is left alone, one view cannot measure it). The nose
#     wing's move is mirrored onto the other wing.
# Each landmark's total move over all rounds is capped at MAX_MM. The moves are spread over
# nearby vertices with Gaussian radial functions (radius per landmark), solved so each
# landmark's own vertices get their move, for ROUNDS rounds. Not used: pupils (eye
# rotation), brows (paint), the ear silhouette point and the lids (the eye opening is
# measured on the eyeball; moving lid vertices sideways does not change it, and changing
# it needs lid topology work, out of scope here).
import json
import os
import sys

import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import camfit  # noqa: E402
import landmarks as L  # noqa: E402

LAB = os.path.dirname(HERE)
MAX_MM = 10.0
ROUNDS = 3
DEPTH_COST, HEIGHT_COST = 1500.0, 200.0     # px per metre of move (see the ridge solve)
MIDLINE = {"nose_tip", "subnasale", "lip_upper", "stomion", "lip_lower", "chin"}
# radius (mm) of each landmark's influence; small round the lids so the other lid stays
RADIUS = {"nose_tip": 14, "alar_R": 10, "subnasale": 9, "chin": 26,
          "mouth_R": 12, "mouth_L": 12, "lip_upper": 8, "stomion": 7, "lip_lower": 8}

head = bpy.data.objects["Bram_Head"]
me = head.data
if me.shape_keys is None:
    head.shape_key_add(name="Basis", from_mix=False)
kb = me.shape_keys.key_blocks.get("Fit_Concept") or head.shape_key_add(name="Fit_Concept", from_mix=False)
n = len(me.vertices)
basis = np.empty(n * 3, dtype=np.float32)
me.shape_keys.key_blocks["Basis"].data.foreach_get("co", basis)
basis = basis.reshape(-1, 3).astype(np.float64)
co = basis.copy()

ids = json.load(open(os.path.join(LAB, "data", "landmarks_model.json")))
fitj = json.load(open(os.path.join(LAB, "data", "camera_fit.json")))
cam = bpy.data.objects["Cam_Concept_Fit"]
Rc = np.array(cam.matrix_world)[:3, :3]
Cc = np.array(cam.matrix_world)[:3, 3]
lens = cam.data.lens
f_px = lens / camfit.SENSOR_H * camfit.H
mw = np.array(head.matrix_world)
assert np.allclose(mw[:3, :3], np.eye(3) * mw[0, 0], atol=1e-6), "head must have uniform scale, no rotation"
scale = mw[0, 0]
names = [k for k in RADIUS]
target = {k: np.array(L.CONCEPT[k][0]) for k in names}


def driver_vertices(k, P):
    """The vertices a landmark moves: its own vertex ids, or for eye-opening points the
    head vertices within 2 mm of the aperture point."""
    v = ids[k]
    if isinstance(v, int):
        return [v]
    if v[0] == "aperture":
        wpt = P[k]
        d = np.linalg.norm(co * scale + mw[:3, 3] - wpt, axis=1)
        return list(np.nonzero(d < 0.002)[0]) or [int(np.argmin(d))]
    return [int(i) for i in v]


def tri_normals(c):
    out = []
    for p in me.polygons:
        vs = c[list(p.vertices)]
        nrm = np.cross(vs[1] - vs[0], vs[2] - vs[0])
        out.append(nrm / (np.linalg.norm(nrm) + 1e-15))
    return np.array(out)


def mirror_vertex(vi):
    m = basis[vi] * np.array([-1, 1, 1])
    return int(np.argmin(((basis - m) ** 2).sum(1)))


def image_jacobian(p):
    """d(pixel)/d(world point), 2x3, numerically."""
    J = np.zeros((2, 3))
    base_px, _ = camfit.project(p[None, :], Rc, Cc, lens)
    for a in range(3):
        q = p.copy()
        q[a] += 1e-5
        J[:, a] = (camfit.project(q[None, :], Rc, Cc, lens)[0][0] - base_px[0]) / 1e-5
    return J


n0 = tri_normals(basis)
total = {k: np.zeros(3) for k in names}
report = []
for rnd in range(ROUNDS):
    P = L.model_points(bpy, ids, co.astype(np.float32))
    centers, moves, radii = [], [], []
    errs = {}
    for k in names:
        pr, depth = camfit.project(P[k][None, :], Rc, Cc, lens)
        e = target[k] - pr[0]
        errs[k] = float(np.linalg.norm(e))
        if k in MIDLINE:
            J = image_jacobian(P[k])[:, 1:]                   # y, z only
            # ridge: a depth (y) move costs as much as DEPTH_COST px of error per metre,
            # height (z) little, so the fit prefers moves the one view can measure
            # (the ridge is on the landmark's total move over all rounds, not per round)
            prev = total[k][1:] * scale
            A = np.vstack([J, np.diag([DEPTH_COST, HEIGHT_COST])])
            yz = np.linalg.lstsq(A, np.r_[e + J @ prev, 0, 0], rcond=None)[0]
            mv = np.array([0.0, yz[0] - prev[0], yz[1] - prev[1]]) / scale
        else:
            mv = (e[0] * Rc[:, 0] - e[1] * Rc[:, 1]) * depth[0] / f_px / scale   # local units (m)
        # cap the total move of this landmark over all rounds
        t = total[k] + mv
        if np.linalg.norm(t) * 1000 > MAX_MM:
            t *= MAX_MM / (np.linalg.norm(t) * 1000)
        mv = t - total[k]
        total[k] = t
        for vi in driver_vertices(k, P):
            centers.append(co[vi])
            moves.append(mv)
            radii.append(RADIUS[k] / 1000.0)
            if k == "alar_R":
                mj = mirror_vertex(vi)
                centers.append(co[mj])
                moves.append(mv * np.array([-1, 1, 1]))
                radii.append(RADIUS[k] / 1000.0)
    report.append({"round": rnd, "err_px": {k: round(v, 1) for k, v in errs.items()}})
    centers, moves, radii = np.array(centers), np.array(moves), np.array(radii)
    # solve RBF weights so the field reproduces each move at its centre
    D = np.linalg.norm(centers[:, None, :] - centers[None, :, :], axis=2)
    K = np.exp(-0.5 * (D / radii[None, :]) ** 2)
    W = np.linalg.solve(K + 1e-6 * np.eye(len(K)), moves)
    Dv = np.linalg.norm(co[:, None, :] - centers[None, :, :], axis=2)
    field = np.exp(-0.5 * (Dv / radii[None, :]) ** 2) @ W
    # keep the field to the front of the face
    field *= np.clip((-co[:, 1] - 0.02) / 0.02, 0, 1)[:, None]
    co = co + field

P = L.model_points(bpy, ids, co.astype(np.float32))
final = {}
allnames = list(L.CONCEPT)
for k in allnames:
    pr, _ = camfit.project(P[k][None, :], Rc, Cc, lens)
    final[k] = float(np.linalg.norm(np.array(L.CONCEPT[k][0]) - pr[0]))
n1 = tri_normals(co)
flipped = int(np.sum(np.einsum("ij,ij->i", n0, n1) < 0))
turned = int(np.sum(np.einsum("ij,ij->i", n0, n1) < np.cos(np.radians(45))))
disp = np.linalg.norm(co - basis, axis=1) * scale * 1000
kb.data.foreach_set("co", co.astype(np.float32).ravel())
kb.value = 0.0
esep = L.eye_separation()
out = {"max_total_move_per_landmark_mm": MAX_MM, "landmark_total_move_mm": {k: np.round(v * scale * 1000, 1).tolist() for k, v in total.items()}, "rounds": ROUNDS, "radius_mm": RADIUS,
       "rounds_log": report,
       "final_err_px": {k: round(v, 1) for k, v in final.items()},
       "final_err_pct_eye_sep": {k: round(100 * v / esep, 1) for k, v in final.items()},
       "used_rms_px": round(float(np.sqrt(np.mean([final[k] ** 2 for k in names]))), 2),
       "anchor_rms_px": round(float(np.sqrt(np.mean([final[k] ** 2 for k in allnames if L.CONCEPT[k][2] == "anchor"]))), 2),
       "vertices_moved_over_0.5mm": int((disp > 0.5).sum()), "max_vertex_move_mm": round(float(disp.max()), 2),
       "faces_flipped": flipped, "faces_turned_over_45deg": turned}
L.save(os.path.join(LAB, "data", "geometry_fit.json"), out)
# landmark JSON in the same layout as camera_fit.json, for the overlay tool
lm = json.loads(json.dumps(fitj))
pts = {}
for k in allnames:
    pr, d = camfit.project(P[k][None, :], Rc, Cc, lens)
    lm["landmarks"][k]["model"] = pr[0].round(1).tolist()
    lm["landmarks"][k]["err_px"] = round(final[k], 1)
    lm["landmarks"][k]["err_pct_eye_sep"] = round(100 * final[k] / esep, 1)
anc = [k for k in allnames if L.CONCEPT[k][2] == "anchor"]
lm["anchor_rms_px"] = out["anchor_rms_px"]
lm["anchor_rms_pct_eye_sep"] = round(100 * out["anchor_rms_px"] / esep, 2)
L.save(os.path.join(LAB, "data", "camera_fit_C.json"), lm)
bpy.ops.wm.save_mainfile(compress=True)
print(json.dumps({k: out[k] for k in ("used_rms_px", "anchor_rms_px", "vertices_moved_over_0.5mm", "max_vertex_move_mm", "faces_flipped", "faces_turned_over_45deg")}))
for r in report:
    print(r)
print("final", out["final_err_px"])
