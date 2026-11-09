# Step 1. Make the face-only working file from a COPY of the Bram lab's final source.
#   blender -b --disable-autoexec ../blender-bram-lab/bram_bust.blend --python-exit-code 1 \
#       --python scripts/make_face_file.py -- <abs path>/face_fit.blend
# The source file is opened read-only and saved under a new name; it is never written.
# Kept: Bram_Head (shape keys and modifiers as they are), the two eyes, the hidden
# original base head for reference. Removed: hair, beard, brows, cap, clothes.
# The head's Mask modifier (collar cut) is kept; its Subdivision modifier is removed so
# the head stays near 25k triangles (report_budget.py). The old skin material and its
# 2048 image are removed (the arms use their own layers).
import sys

import bpy

out = sys.argv[sys.argv.index("--") + 1]
KEEP = {"Bram_Head", "Bram_Eye_L", "Bram_Eye_R", "HBM_Head_Original"}

sc = bpy.data.scenes["BramBust"]
for o in list(bpy.data.objects):
    keep = o.name in KEEP or (o.parent and o.parent.name in KEEP and o.type == "MESH")
    if o.type == "MESH" and not keep:
        bpy.data.objects.remove(o, do_unlink=True)
    elif o.type in {"CURVES", "CURVE", "EMPTY"} and not keep:
        bpy.data.objects.remove(o, do_unlink=True)
head = bpy.data.objects["Bram_Head"]
for m in list(head.modifiers):
    if m.type == "SUBSURF":
        head.modifiers.remove(m)
head.data.materials.clear()
for name in ("M_Bram_Skin",):
    if name in bpy.data.materials:
        bpy.data.materials.remove(bpy.data.materials[name])
if "T_Bram_Skin" in bpy.data.images:
    bpy.data.images.remove(bpy.data.images["T_Bram_Skin"])
for coll in (bpy.data.meshes, bpy.data.materials, bpy.data.curves, bpy.data.hair_curves
             if hasattr(bpy.data, "hair_curves") else []):
    for b in list(coll):
        if b.users == 0 and not b.use_fake_user:
            coll.remove(b)

sc.name = "FaceFit"
bpy.ops.wm.save_as_mainfile(filepath=out, compress=True, relative_remap=True, copy=False)
print("saved face file; objects:", sorted(o.name for o in bpy.data.objects))
