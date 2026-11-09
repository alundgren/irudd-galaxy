# Put one arm on the head and save, so the file opens showing it (default B).
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/set_arm.py -- [A|B|C]
import sys

import bpy

which = (sys.argv[sys.argv.index("--") + 1:] or ["B"])[0] if "--" in sys.argv else "B"
head = bpy.data.objects["Bram_Head"]
mat, eye, ink, key = {"A": ("M_Clay", "M_Clay_Eye", False, 0.0), "B": ("M_Face_Cel", "M_Eye_Cel", True, 0.0),
                      "C": ("M_Face_Cel_C", "M_Eye_Cel", True, 1.0)}[which]
head.data.materials.clear()
head.data.materials.append(bpy.data.materials[mat])
head.data.materials.append(bpy.data.materials["M_Ink"])
for e in ("Bram_Eye_L", "Bram_Eye_R"):
    bpy.data.objects[e].data.materials.clear()
    bpy.data.objects[e].data.materials.append(bpy.data.materials[eye])
head.modifiers["Ink_Hull"].show_render = head.modifiers["Ink_Hull"].show_viewport = ink
head.data.shape_keys.key_blocks["Fit_Concept"].value = key
sc = bpy.context.scene
sc.camera = bpy.data.objects["Cam_Concept_Fit"]
sc.render.resolution_x, sc.render.resolution_y = 568, 900
for c in bpy.data.collections:
    if c.name.startswith(("Rig_", "Bram_Rig_")):
        c.hide_render = c.name != "Rig_Concept"
        for o in c.objects:
            if o.type == "LIGHT":
                o.hide_render = c.name != "Rig_Concept"
sc.world = bpy.data.worlds.get("W_Lab") or sc.world
# viewport: look through the fitted camera in material preview
for scr in bpy.data.screens:
    for area in scr.areas:
        if area.type == "VIEW_3D":
            sp = area.spaces[0]
            sp.region_3d.view_perspective = "CAMERA"
            sp.shading.type = "MATERIAL"
bpy.ops.wm.save_mainfile(compress=True)
print("arm", which)
