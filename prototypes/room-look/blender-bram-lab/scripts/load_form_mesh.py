# Build (or replace the geometry of) an editable mesh object from a build_form_mesh .npz.
import bpy
import numpy as np


def load(npz_path, name, collection=None):
    data = np.load(npz_path)
    co, totals, verts = data["co"], data["loop_totals"], data["verts"]
    me = bpy.data.meshes.new(name + "_mesh")
    me.vertices.add(len(co))
    me.vertices.foreach_set("co", co.ravel())
    me.loops.add(len(verts))
    me.loops.foreach_set("vertex_index", verts)
    me.polygons.add(len(totals))
    starts = np.concatenate([[0], np.cumsum(totals)[:-1]]).astype(np.int32)
    me.polygons.foreach_set("loop_start", starts)
    me.update(calc_edges=True)
    me.validate()
    me.shade_smooth()
    obj = bpy.data.objects.get(name)
    if obj is None:
        obj = bpy.data.objects.new(name, me)
        (collection or bpy.context.scene.collection).objects.link(obj)
    else:
        old = obj.data
        obj.data = me
        for m in old.materials:
            me.materials.append(m)
        bpy.data.meshes.remove(old)
    me.name = name + "_mesh"
    return obj
