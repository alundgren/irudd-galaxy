# Render the authored board and its glTF export under identical conditions.
# Run in background; it never saves the .blend:
#   blender -b board_corner.blend --python reimport_check.py -- <out_dir> [prefix]
# Source and reimport live in separate scenes. The reimport scene is a copy of the
# authored scene (same camera, lights, world, render and colour settings) with the
# authored asset collection unlinked and the GLB imported in its place.
import os
import sys

import bpy

here = os.path.dirname(bpy.data.filepath)
args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
out_dir = args[0] if args else os.path.join(here, "renders")
prefix = args[1] if len(args) > 1 else "export"
glb = os.path.join(here, "board_corner.glb")
src = bpy.data.scenes["BoardCorner"]

chk = src.copy()
chk.name = "GLB_Reimport_Check"
asset = bpy.data.collections["BoardCorner_Asset"]
chk.collection.children.unlink(asset)
with bpy.context.temp_override(scene=chk, view_layer=chk.view_layers[0]):
    bpy.ops.import_scene.gltf(filepath=glb)
imported = [o for o in chk.objects if o.type == "MESH"]
print("reimported meshes:", len(imported))


def neutral(scene, on):
    bg = next(n for n in scene.world.node_tree.nodes if n.type == "BACKGROUND")
    if on:
        neutral.saved = (tuple(bg.inputs[0].default_value), bg.inputs[1].default_value)
        bg.inputs[0].default_value = (1, 1, 1, 1)
        bg.inputs[1].default_value = 1.0
    else:
        bg.inputs[0].default_value, bg.inputs[1].default_value = neutral.saved
    for o in scene.objects:
        if o.type == "LIGHT":
            o.hide_render = on


for label, scene in (("src", src), ("glb", chk)):
    for cam, name in (("Cam_Reference", "reference_lit"), ("Cam_Oblique_LeftHigh", "oblique_left_high"),
                      ("Cam_Oblique_RightLow", "oblique_right_low")):
        scene.camera = bpy.data.objects[cam]
        scene.render.filepath = os.path.join(out_dir, f"{prefix}_{label}_{name}.png")
        bpy.ops.render.render(write_still=True, scene=scene.name)
    scene.camera = bpy.data.objects["Cam_Reference"]
    neutral(scene, True)
    scene.render.filepath = os.path.join(out_dir, f"{prefix}_{label}_reference_neutral.png")
    bpy.ops.render.render(write_still=True, scene=scene.name)
    neutral(scene, False)
    vs = scene.view_settings
    print(label, scene.render.engine, vs.view_transform, vs.look, vs.exposure, vs.gamma,
          scene.render.resolution_x, scene.render.resolution_y)

# glTF has no "cast no shadow" flag. The ink outline shell therefore shadows its own part
# after import. Re-render the imported copy with that one renderer-side setting restored
# to show whether it explains the whole lit difference.
src_ink = bpy.data.materials["M_Ink_Outline"]
for o in imported:
    for slot in o.material_slots:
        m = slot.material
        if m and m.name.startswith("M_Ink_Outline") and m != src_ink:
            m.use_backface_culling_shadow = True
for cam, name in (("Cam_Reference", "reference_lit"), ("Cam_Oblique_LeftHigh", "oblique_left_high"),
                  ("Cam_Oblique_RightLow", "oblique_right_low")):
    chk.camera = bpy.data.objects[cam]
    chk.render.filepath = os.path.join(out_dir, f"{prefix}_glb_hullnoshadow_{name}.png")
    bpy.ops.render.render(write_still=True, scene=chk.name)
