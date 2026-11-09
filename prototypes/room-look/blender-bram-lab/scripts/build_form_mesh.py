# Turn a distance-field form into a triangle/quad mesh, outside the live Blender session.
#   blender -b --factory-startup --python build_form_mesh.py -- <form_module> <voxel_mm> <out.npz> <work_dir>
# It samples the field, stores it in an OpenVDB grid, meshes it with Blender's
# Volume to Mesh modifier and writes vertices and faces. The live session then builds
# an ordinary editable mesh from the .npz, so no volume file stays referenced.
import importlib
import os
import sys
import time

import bpy
import numpy as np
import openvdb

args = sys.argv[sys.argv.index("--") + 1:]
module_name, voxel_mm, out_path, work_dir = args[0], float(args[1]), args[2], args[3]
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
form = importlib.import_module(module_name)
vs = voxel_mm / 1000.0
(x0, x1), (y0, y1), (z0, z1) = form.BOUNDS
xs = np.arange(x0, x1, vs, dtype=np.float32)
ys = np.arange(y0, y1, vs, dtype=np.float32)
zs = np.arange(z0, z1, vs, dtype=np.float32)
field = np.zeros((len(xs), len(ys), len(zs)), dtype=np.float32)
t0 = time.time()
fn = getattr(form, getattr(form, "FIELD", "head"))
X2, Y2 = np.meshgrid(xs, ys, indexing="ij")
slab = 24
for k in range(0, len(zs), slab):
    zz = zs[k:k + slab]
    X = np.repeat(X2[:, :, None], len(zz), axis=2)
    Y = np.repeat(Y2[:, :, None], len(zz), axis=2)
    Z = np.broadcast_to(zz[None, None, :], X.shape).copy()
    field[:, :, k:k + len(zz)] = fn(X, Y, Z)
print("field", field.shape, round(time.time() - t0, 1), "s")
# Positive inside, in voxel units, clamped so the outside equals the background (0).
val = np.clip(-field / vs, -3.0, 3.0) + 3.0
grid = openvdb.FloatGrid()
grid.copyFromArray(val.astype(np.float32))
grid.name = "density"
grid.transform = openvdb.createLinearTransform(voxelSize=vs)
vdb_path = os.path.join(work_dir, module_name + ".vdb")
openvdb.write(vdb_path, grids=[grid])

bpy.ops.object.volume_import(filepath=vdb_path)
vol = bpy.context.active_object
bpy.ops.mesh.primitive_plane_add()
obj = bpy.context.active_object
mod = obj.modifiers.new("v2m", "VOLUME_TO_MESH")
mod.object = vol
mod.grid_name = "density"
mod.threshold = 3.0
mod.resolution_mode = "GRID"
mod.adaptivity = 0.0
mod.use_smooth_shade = True
dg = bpy.context.evaluated_depsgraph_get()
ev = obj.evaluated_get(dg)
me = ev.to_mesh()
n = len(me.vertices)
co = np.empty(n * 3, dtype=np.float32)
me.vertices.foreach_get("co", co)
co = co.reshape(-1, 3) + np.array([x0, y0, z0], dtype=np.float32)
loop_totals = np.empty(len(me.polygons), dtype=np.int32)
me.polygons.foreach_get("loop_total", loop_totals)
verts = np.empty(len(me.loops), dtype=np.int32)
me.loops.foreach_get("vertex_index", verts)
np.savez_compressed(out_path, co=co, loop_totals=loop_totals, verts=verts)
print("mesh", n, "verts", len(loop_totals), "faces", round(time.time() - t0, 1), "s")
os.remove(vdb_path)
