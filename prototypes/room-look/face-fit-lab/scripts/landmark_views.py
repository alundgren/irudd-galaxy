# Diagnostic: the 3D landmark points as small markers on the clay head, seen from the
# front, his right profile, below, and through the fitted camera. Nothing is saved.
#   blender -b face_fit.blend --python scripts/landmark_views.py -- <out_dir> <tag> [shape key]
import json, os, sys
import bpy
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import landmarks as L, scene as S
args = sys.argv[sys.argv.index("--") + 1:]
out, tag = args[0], args[1]
key = args[2] if len(args) > 2 else None
head = bpy.data.objects[S.HEAD]
if head.data.shape_keys:
    for kb in head.data.shape_keys.key_blocks[1:]:
        kb.value = 1.0 if kb.name == key else 0.0
ids = json.load(open(os.path.join(os.path.dirname(HERE), "data", "landmarks_model.json")))
coords = None
if key:
    kb = head.data.shape_keys.key_blocks[key]
    coords = np.array([v.co[:] for v in kb.data])
P = L.model_points(bpy, ids, coords)
cols = {"anchor": (0, 1, 1, 1), "form": (1, 0.35, 0.05, 1), "expr": (1, 0.8, 0, 1), "paint": (0.4, 1, 0.3, 1)}
for k, p in P.items():
    role = L.CONCEPT[k][2]
    m = bpy.data.materials.get("M_Mark_" + role) or bpy.data.materials.new("M_Mark_" + role)
    m.use_nodes = True
    b = S.principled(m); b.inputs["Base Color"].default_value = cols[role]; b.inputs["Emission Color"].default_value = cols[role]; b.inputs["Emission Strength"].default_value = 2.0
    bpy.ops.mesh.primitive_uv_sphere_add(radius=0.0018, location=tuple(p), segments=12, ring_count=8)
    o = bpy.context.active_object; o.name = "Mark_" + k; o.data.materials.append(m)
S.set_materials(S.clay_material(), S.eye_clay_material())
if head.modifiers.get("Ink_Hull"):
    head.modifiers["Ink_Hull"].show_render = False
cam = bpy.data.objects["Cam_Concept_Fit"]
S.make_rigs(cam); S.use_rig("clay"); S.world()
views = {"front": (0, -1, 0), "profile_right": (-1, 0, 0), "below": (0, -0.55, -0.83)}
for v, d in views.items():
    c = S.axis_camera("Cam_tmp_" + v, d, dist=0.75, lens=100)
    S.render(c, os.path.abspath(os.path.join(out, f"{tag}_marks_{v}.png")), 600, 600)
S.render(cam, os.path.abspath(os.path.join(out, f"{tag}_marks_concept_cam.png")), 568, 900)
