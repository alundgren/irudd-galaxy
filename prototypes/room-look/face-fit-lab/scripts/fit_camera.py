# Step 2. Fit the concept camera to the anchor landmarks and save it as Cam_Concept_Fit.
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/fit_camera.py
# Writes data/landmarks_model.json (vertex ids) and data/camera_fit.json (camera, errors,
# lens sweep). Only "anchor" landmarks drive the fit; the others are reported.
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
os.makedirs(os.path.join(LAB, "data"), exist_ok=True)

ids = L.pick_model_ids(bpy)
L.save(os.path.join(LAB, "data", "landmarks_model.json"), ids)
P3 = L.model_points(bpy, ids)
names = list(L.CONCEPT)
uv = np.array([L.CONCEPT[k][0] for k in names])
sig = np.array([L.CONCEPT[k][1] for k in names], dtype=float)
role = [L.CONCEPT[k][2] for k in names]
P = np.array([P3[k] for k in names])
anc = np.array([r == "anchor" for r in role])
esep = L.eye_separation()
# Portrait lens range. Wider lenses fit the anchors slightly better (see lens_sweep) by
# exaggerating perspective, which would hide proportion differences in the camera.
LENS = (50.0, 150.0)

# starts: the old hand-placed goal camera, plus a sweep of orbit angles round the face
head_c = np.array([0.0, -0.06, -0.02])
starts = []
g = bpy.data.objects["Cam_Goal_Tilt"]
starts.append(np.r_[camfit.mat_to_rotvec(np.array(g.matrix_world)[:3, :3]), np.array(g.matrix_world)[:3, 3], 80.0])
for az in range(-80, -9, 10):           # camera on his right side (-X), degrees from front
    for el in (-10, 0, 10):
        for roll in (-25, -15, -5, 5):
            a, e = np.radians(az), np.radians(el)
            d = 0.9
            C = head_c + d * np.array([np.sin(a), -np.cos(a) * np.cos(e), np.sin(e)])
            R = camfit.look_at(C, head_c, roll)
            starts.append(np.r_[camfit.mat_to_rotvec(R), C, 80.0])


def cost(p, mask):
    R, C, lens = camfit._unpack(p)
    pr, _ = camfit.project(P[mask], R, C, lens)
    return float(np.sqrt(np.mean(np.sum((pr - uv[mask]) ** 2, 1))))


best = None
for s in starts:
    p, _ = camfit.fit(P[anc], uv[anc], sig[anc], s, lens_bounds=LENS, iters=80)
    c = cost(p, anc)
    if best is None or c < best[1]:
        best = (p, c)
p, _ = camfit.fit(P[anc], uv[anc], sig[anc], best[0], lens_bounds=LENS, iters=300)
R, C, lens = camfit._unpack(p)

# Aim both eyes at the fitted camera (the concept looks at the viewer). This turns the
# eye meshes only; the head is unchanged.
from mathutils import Vector  # noqa: E402
for eye in ("Bram_Eye_R", "Bram_Eye_L"):
    o = bpy.data.objects[eye]
    c = Vector(o.matrix_world.translation)
    d0 = (Vector(L.pupil_world(bpy, eye)) - c).normalized()
    d1 = (Vector(C) - c).normalized()
    q = d0.rotation_difference(d1)
    o.rotation_mode = "QUATERNION"
    o.rotation_quaternion = q @ o.matrix_world.to_quaternion()
    bpy.context.view_layer.update()
P3 = L.model_points(bpy, ids)
P = np.array([P3[k] for k in names])
pr, depth = camfit.project(P, R, C, lens)
err = np.linalg.norm(pr - uv, axis=1)

# for comparison: a plain least-squares fit with the form points (nose, chin) added as anchors
form = anc | np.array([r == "form" for r in role])
best_f = None
for s0 in [p] + starts[::7]:
    q, _ = camfit.fit(P[form], uv[form], sig[form], s0, lens_bounds=LENS, iters=150, huber=1e9)
    Rq, Cq, lq = camfit._unpack(q)
    cq = float(np.sum(((camfit.project(P[form], Rq, Cq, lq)[0] - uv[form]) / sig[form][:, None]) ** 2))
    if best_f is None or cq < best_f[1]:
        best_f = (q, cq)
pf = best_f[0]
Rf, Cf, lf = camfit._unpack(pf)
prf, _ = camfit.project(P, Rf, Cf, lf)
ef = np.linalg.norm(prf - uv, axis=1)
alt = {"lens_mm": round(float(lf), 1), "anchor_rms_px": round(float(np.sqrt(np.mean(ef[anc] ** 2))), 2),
       "anchor_plus_form_rms_px": round(float(np.sqrt(np.mean(ef[form] ** 2))), 2),
       "rotation_difference_deg": round(float(np.degrees(np.linalg.norm(camfit.mat_to_rotvec(R.T @ Rf)))), 1)}

# lens sweep: refit with the lens held fixed, to show the depth / focal ambiguity
sweep = []
for fl in (35, 50, 80, 120, 200):
    q, _ = camfit.fit(P[anc], uv[anc], sig[anc], p, fix_lens=float(fl), iters=200)
    Rq, Cq, _ = camfit._unpack(q)
    prq, dq = camfit.project(P, Rq, Cq, fl)
    eq = np.linalg.norm(prq - uv, axis=1)
    sweep.append({"lens_mm": fl, "distance_m": round(float(np.linalg.norm(Cq - head_c)), 3),
                  "anchor_rms_px": round(float(np.sqrt(np.mean(eq[anc] ** 2))), 2),
                  "all_rms_px": round(float(np.sqrt(np.mean(eq ** 2))), 2)})

# orientation of the camera relative to the head, for the report
fwd = -R[:, 2]
yaw = float(np.degrees(np.arctan2(-fwd[0], fwd[1])))     # 0 = looking at his face from the front
pitch = float(np.degrees(np.arcsin(-fwd[2])))
out = {
    "crop": L.CROP, "image_size": [camfit.W, camfit.H], "eye_separation_px": round(esep, 1),
    "camera": {"matrix_world_3x3": R.round(6).tolist(), "location_m": C.round(5).tolist(),
               "lens_mm": round(float(lens), 2), "sensor_fit": "VERTICAL", "sensor_height_mm": 36.0,
               "distance_to_face_m": round(float(np.linalg.norm(C - head_c)), 3),
               "yaw_deg_from_front_toward_his_right": round(-yaw, 1), "looks_up_deg": round(-pitch, 1)},
    "anchor_rms_px": round(float(np.sqrt(np.mean(err[anc] ** 2))), 2),
    "anchor_rms_pct_eye_sep": round(float(100 * np.sqrt(np.mean(err[anc] ** 2)) / esep), 2),
    "anchor_worst": [names[int(np.argmax(np.where(anc, err, -1)))], round(float(np.max(err[anc])), 2)],
    "all_rms_px": round(float(np.sqrt(np.mean(err ** 2))), 2),
    "landmarks": {k: {"concept": uv[i].tolist(), "model": pr[i].round(1).tolist(), "err_px": round(float(err[i]), 1),
                      "err_pct_eye_sep": round(float(100 * err[i] / esep), 1), "conf_px": sig[i], "role": role[i],
                      "depth_m": round(float(depth[i]), 4)} for i, k in enumerate(names)},
    "lens_sweep": sweep,
    "fit_with_form_points_too": alt,
    "form_rms_px": round(float(np.sqrt(np.mean(err[form & ~anc] ** 2))), 2),
}
L.save(os.path.join(LAB, "data", "camera_fit.json"), out)

cam = bpy.data.objects.get("Cam_Concept_Fit")
if cam is None:
    cam = bpy.data.objects.new("Cam_Concept_Fit", bpy.data.cameras.new("Cam_Concept_Fit"))
    bpy.context.scene.collection.objects.link(cam)
M = np.eye(4)
M[:3, :3] = R
M[:3, 3] = C
from mathutils import Matrix  # noqa: E402
cam.matrix_world = Matrix(M.tolist())
cam.data.lens = float(lens)
cam.data.sensor_fit = "VERTICAL"
cam.data.sensor_height = 36.0
cam.data.clip_start = 0.01
sc = bpy.context.scene
sc.camera = cam
sc.render.resolution_x, sc.render.resolution_y = camfit.W, camfit.H
bpy.ops.wm.save_mainfile(compress=True)
print(json.dumps({k: out[k] for k in ("camera", "anchor_rms_px", "anchor_rms_pct_eye_sep", "anchor_worst", "form_rms_px", "all_rms_px", "lens_sweep", "fit_with_form_points_too")}, indent=1))
for k, v in out["landmarks"].items():
    print(f"{k:14s} {v['role']:6s} err {v['err_px']:6.1f}px {v['err_pct_eye_sep']:5.1f}%  conf {v['conf_px']}")
