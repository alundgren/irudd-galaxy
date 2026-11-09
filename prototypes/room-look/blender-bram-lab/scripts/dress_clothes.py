# Colours and trim for the jacket, collar and shirt: navy body, a teal yoke over the
# shoulders, coral piping along the yoke and the jacket front, mustard collar with a
# coral edge, cream shirt. Run inside Blender after the cloth meshes exist.
import math

import bmesh
import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def material(name, rgb, rough, sheen=0.0):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = (*rgb, 1)
    b.inputs["Roughness"].default_value = rough
    return m


NAVY = material("M_Bram_Jacket_Navy", (0.030, 0.032, 0.095), 0.88)
TEAL = material("M_Bram_Jacket_Teal", (0.030, 0.150, 0.150), 0.85)
CORAL = material("M_Bram_Coral", (0.62, 0.20, 0.16), 0.6)
MUSTARD = material("M_Bram_Collar_Mustard", (0.62, 0.33, 0.050), 0.8)
CREAM = material("M_Bram_Shirt_Cream", (0.66, 0.58, 0.42), 0.85)

YOKE_Z = -0.345   # lower edge of the teal yoke


def set_materials(obj, mats, index_of_face=None):
    me = obj.data
    me.materials.clear()
    for m in mats:
        me.materials.append(m)
    if index_of_face is not None:
        n = len(me.polygons)
        c = np.empty(n * 3, dtype=np.float32)
        me.polygons.foreach_get("center", c)
        idx = index_of_face(c.reshape(-1, 3)).astype(np.int32)
        me.polygons.foreach_set("material_index", idx)
    me.update()


def tube_object(name, pts, radius, mat, cyclic=False):
    cu = bpy.data.curves.new(name + "_curve", "CURVE")
    cu.dimensions = "3D"
    sp = cu.splines.new("POLY")
    sp.points.add(len(pts) - 1)
    for p, q in zip(sp.points, pts):
        p.co = (q.x, q.y, q.z, 1)
    sp.use_cyclic_u = cyclic
    cu.bevel_depth = radius
    cu.bevel_resolution = 2
    tmp = bpy.data.objects.new("tmp_" + name, cu)
    bpy.data.collections["Bram_Asset"].objects.link(tmp)
    me = bpy.data.meshes.new_from_object(tmp.evaluated_get(bpy.context.evaluated_depsgraph_get()))
    bpy.data.objects.remove(tmp)
    bpy.data.curves.remove(cu)
    old = bpy.data.objects.get(name)
    if old:
        om = old.data
        old.data = me
        bpy.data.meshes.remove(om)
        obj = old
    else:
        obj = bpy.data.objects.new(name, me)
        bpy.data.collections["Bram_Asset"].objects.link(obj)
    me.materials.clear()
    me.materials.append(mat)
    me.shade_smooth()
    return obj


def bvh(obj):
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    t = BVHTree.FromBMesh(bm)
    bm.free()
    return t


def surface_ring(tree, z, centre=(0.0, 0.02), pad=0.0012, arc=(-math.pi, math.pi), n=180):
    pts = []
    for i in range(n):
        th = arc[0] + (arc[1] - arc[0]) * i / (n - 1)
        d = Vector((math.sin(th), -math.cos(th), 0.0))
        o = Vector((centre[0], centre[1], z)) + d * 0.5
        hit, nrm, *_ = tree.ray_cast(o, -d)
        if hit is not None:
            pts.append(hit + nrm * pad)
    return pts


def dress():
    jacket = bpy.data.objects["Bram_Jacket"]
    collar = bpy.data.objects["Bram_Collar"]
    shirt = bpy.data.objects["Bram_Shirt"]

    def jacket_index(c):
        x, y, z = c[:, 0], c[:, 1], c[:, 2]
        yoke = (z > YOKE_Z) & ~((np.abs(x) > 0.17) & (z < -0.30))
        return np.where(yoke, 0, 1)
    set_materials(jacket, [NAVY, TEAL], jacket_index)
    set_materials(collar, [MUSTARD])
    set_materials(shirt, [CREAM])
    for o in (jacket, collar, shirt):
        o.data.shade_smooth()

    jt = bvh(jacket)
    # Piping along the yoke edge, front and back, broken where the jacket opens
    front = surface_ring(jt, YOKE_Z, arc=(-math.pi * 0.95, -0.42), n=120)
    front2 = surface_ring(jt, YOKE_Z, arc=(0.42, math.pi * 0.95), n=120)
    tube_object("Bram_Jacket_Piping_YokeR", front, 0.0022, CORAL)
    tube_object("Bram_Jacket_Piping_YokeL", front2, 0.0022, CORAL)
    # Piping down each front edge of the jacket
    for sx, tag in ((-1, "R"), (1, "L")):
        pts = []
        for i in range(60):
            zz = -0.215 - 0.30 * i / 59
            half = min(max(0.060 * (zz + 0.430) / 0.200, 0.0), 0.075) + 0.012
            o = Vector((sx * (half + 0.006), -0.40, zz))
            hit, nrm, *_ = jt.ray_cast(o, Vector((0, 1, 0)))
            if hit is not None:
                pts.append(hit + nrm * 0.0015)
        if len(pts) > 2:
            tube_object("Bram_Jacket_Piping_Front" + tag, pts, 0.0024, CORAL)
    # Coral edge on the collar's upper rim
    ct = bvh(collar)
    me = collar.data
    co = np.empty(len(me.vertices) * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    for sx, tag in ((-1, "R"), (1, "L")):
        pts = []
        for i in range(90):
            th = sx * (0.35 + (math.pi - 0.35) * i / 89)
            d = np.array([math.sin(th) * 1.08, -math.cos(th)])
            rel = co[:, :2] - np.array([0.0, 0.020])
            radial = rel @ d / (np.linalg.norm(d) ** 2)
            perp = np.abs(rel[:, 0] * d[1] - rel[:, 1] * d[0]) / np.linalg.norm(d)
            sel = (perp < 0.004) & (radial > 0)
            if sel.sum() == 0:
                continue
            k = np.argmax(np.where(sel, co[:, 2], -9))
            pts.append(Vector(co[k]) + Vector((0, 0, 0.0008)))
        if len(pts) > 2:
            for _ in range(4):
                pts = [pts[0]] + [(pts[i - 1] + pts[i] * 2 + pts[i + 1]) / 4 for i in range(1, len(pts) - 1)] + [pts[-1]]
            tube_object("Bram_Collar_Piping_" + tag, pts, 0.0030, CORAL)
    return "dressed"


print(dress())
