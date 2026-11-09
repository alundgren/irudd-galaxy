# Report triangles (as rendered, modifiers applied), materials and images; writes data/budget.json.
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/report_budget.py
import os
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import landmarks as L  # noqa: E402

LAB = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
dg = bpy.context.evaluated_depsgraph_get()
out = {"objects": {}}
for name in ("Bram_Head", "Bram_Eye_L", "Bram_Eye_R"):
    o = bpy.data.objects[name]
    hull = o.modifiers.get("Ink_Hull")
    row = {}
    for label, on in (("with_ink_shell", True), ("without_ink_shell", False)):
        if hull:
            hull.show_viewport = on
        bpy.context.view_layer.update()
        e = o.evaluated_get(bpy.context.evaluated_depsgraph_get())
        me = e.to_mesh()
        me.calc_loop_triangles()
        row[label] = len(me.loop_triangles)
        e.to_mesh_clear()
    row["base_vertices"] = len(o.data.vertices)
    row["materials"] = [s.material.name for s in o.material_slots if s.material]
    out["objects"][name] = row
out["images"] = {i.name: {"size": list(i.size), "packed": bool(i.packed_file), "fake_user": i.use_fake_user}
                 for i in bpy.data.images if i.name.startswith("T_")}
out["shape_keys"] = [k.name for k in bpy.data.objects["Bram_Head"].data.shape_keys.key_blocks]
L.save(os.path.join(LAB, "data", "budget.json"), out)
print(out)
