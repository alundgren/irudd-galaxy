# Step 6. Render every view of the checkpoint into renders/work/ (the .blend is not saved).
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/render_checkpoint.py -- [only]
# Arms:
#   A        Bram lab head, fitted camera, plain clay, no ink.
#   B        same geometry and camera, T_Face_Paint under 3 cel bands, plum ink shell.
#   C        B's look on the Fit_Concept shape key, paint re-projected onto that geometry.
#   A_clay / C_clay  geometry check in clay.   raw  the raw projected layer, unlit.
import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import scene as S  # noqa: E402

LAB = os.path.dirname(HERE)
OUT = os.path.join(LAB, "renders", "work")
only = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
head = bpy.data.objects[S.HEAD]
hull = head.modifiers["Ink_Hull"]
cam = bpy.data.objects["Cam_Concept_Fit"]
S.make_rigs(cam)
S.world()


def arm(name):
    use_key = name.startswith("C")
    if head.data.shape_keys:
        head.data.shape_keys.key_blocks["Fit_Concept"].value = 1.0 if use_key else 0.0
    mats = {"A": ("M_Clay", "M_Clay_Eye", False), "A_clay": ("M_Clay", "M_Clay_Eye", False),
            "C_clay": ("M_Clay", "M_Clay_Eye", False), "B": ("M_Face_Cel", "M_Eye_Cel", True),
            "C": ("M_Face_Cel_C", "M_Eye_Cel", True), "raw": ("M_Face_Raw", "M_Eye_Cel", False),
            "raw_C": ("M_Face_Raw_C", "M_Eye_Cel", False)}[name]
    S.clay_material()
    S.eye_clay_material()
    me = head.data
    me.materials.clear()
    me.materials.append(bpy.data.materials[mats[0]])
    me.materials.append(bpy.data.materials["M_Ink"])
    for e in S.EYES:
        bpy.data.objects[e].data.materials.clear()
        bpy.data.objects[e].data.materials.append(bpy.data.materials[mats[1]])
    hull.show_render = mats[2]
    bpy.context.view_layer.update()


def shot(armname, rig, view, w=568, h=900):
    if only and armname not in only:
        return
    arm(armname)
    S.use_rig(rig)
    if view == "concept":
        c = cam
    elif view.startswith("yaw"):
        c = S.orbit_camera("Cam_" + view, cam, yaw_deg=float(view[3:]))
    elif view == "front":
        c = S.axis_camera("Cam_Front_Lab", (0, -1, 0), dist=0.9, lens=100)
    elif view == "profile":
        c = S.axis_camera("Cam_Profile_Lab", (-1, 0, 0), dist=0.9, lens=100)
    S.render(c, os.path.join(OUT, f"{armname}_{rig}_{view}.png"), w, h)


for a in ("A", "B", "C"):
    shot(a, "clay" if a == "A" else "concept", "concept")
for a in ("A_clay", "C_clay"):
    for v in ("front", "profile", "yaw-20", "yaw20"):
        shot(a, "clay", v, 600, 700)
    shot(a, "clay", "concept")
for a in ("B", "C"):
    for v in ("yaw-20", "yaw20"):
        shot(a, "concept", v)
    shot(a, "swapped", "concept")
for v in ("concept", "yaw-20", "yaw20"):
    shot("raw", "clay", v)
print("rendered")
