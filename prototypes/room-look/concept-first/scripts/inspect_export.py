"""Print collections, roots, object counts and packed images of an exported .blend, and render it
through a camera looking along +Y at the board slot (partner's room convention) as a check.
usage: blender -b <file.blend> --factory-startup --disable-autoexec --python inspect_export.py -- <out.png>"""
import os, sys, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy
from mathutils import Vector
import cf

args = cf.argv()
sc = bpy.context.scene
for c in bpy.data.collections:
    print("COLL", c.name, len(c.all_objects), "objects")
for n in ("board_root", "porthole_root"):
    o = bpy.data.objects.get(n)
    print("ROOT", n, tuple(round(v, 3) for v in o.location), tuple(round(v, 3) for v in o.scale) if o else None)
print("IMAGES", len(bpy.data.images), "packed", sum(1 for i in bpy.data.images if i.packed_file))
fr = bpy.data.objects["board_frame"]
bb = [fr.matrix_world @ Vector(c) for c in fr.bound_box]
print("FRAME world min", [round(min(v[i] for v in bb), 3) for i in range(3)], "max", [round(max(v[i] for v in bb), 3) for i in range(3)])
camd = bpy.data.cameras.new("chk")
camd.lens = 35
cam = bpy.data.objects.new("chk", camd)
sc.collection.objects.link(cam)
cam.location = (0.89, -1.4, 1.33)
cam.rotation_euler = (math.radians(90), 0, 0)
sc.frame_set(60)
cf.resolution(sc, pct=50)
cf.render(sc, cam, args[0])
