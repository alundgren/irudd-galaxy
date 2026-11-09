# Export the visible bust as a standard glTF binary and print asset statistics.
#   blender -b bram_bust.blend --python-exit-code 1 --python scripts/export_glb.py -- <out.glb>
# Exports the meshes of Bram_Asset that render (hidden cp2 collar and piping and the
# hidden CC0 original are left out), with modifiers applied. No cameras, lights or
# animation. The .blend is not saved.
import os
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lab_common  # noqa: E402

out = sys.argv[sys.argv.index("--") + 1]
objs = lab_common.asset_meshes()
for o in bpy.context.view_layer.objects:
    o.select_set(False)
for o in objs:
    o.hide_set(False)
    o.select_set(True)
bpy.context.view_layer.objects.active = objs[0]

dg = bpy.context.evaluated_depsgraph_get()
total = 0
rows = []
for o in objs:
    me = o.evaluated_get(dg).to_mesh()
    me.calc_loop_triangles()
    t = len(me.loop_triangles)
    total += t
    rows.append((t, o.name, [m.name for m in o.data.materials if m]))
    o.evaluated_get(dg).to_mesh_clear()
for t, name, mats in sorted(rows, reverse=True):
    print(f"STAT {name}: {t} triangles, materials {mats}")
print(f"STAT total: {total} triangles in {len(objs)} meshes")
for im in bpy.data.images:
    if im.users and im.size[0]:
        print(f"STAT image {im.name}: {im.size[0]}x{im.size[1]}, packed {im.packed_file is not None}")

bpy.ops.export_scene.gltf(filepath=out, export_format="GLB", use_selection=True, export_apply=True,
                          export_cameras=False, export_lights=False, export_animations=False,
                          export_yup=True)
print(f"STAT glb bytes: {os.path.getsize(out)}")
