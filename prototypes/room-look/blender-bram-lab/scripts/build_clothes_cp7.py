# Checkpoint 7 clothing: broader shoulders, a turned-down mustard collar round the back of
# the neck, separate folded mustard lapels lying over the open jacket front, shirt collar
# points, broad folds in the jacket, and a painted layer (colour attribute "Paint_Cloth")
# with plum at the joins and folds and warm light on the upper planes.
# Run inside Blender after the cp1-cp6 cloth objects exist. The cp2 funnel collar is kept
# but hidden. Rerunning starts again from the jacket and shirt vertex positions saved the first time.
import math

import bmesh
import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

import sculpt_cp3

COL = "Bram_Asset"
WIDEN = 0.15          # extra shoulder and chest width below the neck
NECK = (0.0, 0.025)   # neck centre (x, y)


def material(name, rgb, rough):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = (*rgb, 1)
    b.inputs["Roughness"].default_value = rough
    return m


MUSTARD = material("M_Bram_Collar_Mustard", (0.62, 0.33, 0.050), 0.8)
CORAL = material("M_Bram_Coral", (0.62, 0.20, 0.16), 0.6)
CREAM = material("M_Bram_Shirt_Cream", (0.66, 0.58, 0.42), 0.85)


def coords(obj):
    c = np.empty(len(obj.data.vertices) * 3, dtype=np.float64)
    obj.data.vertices.foreach_get("co", c)
    return c.reshape(-1, 3)


def keep_original(obj):
    """Store the vertex positions from before cp7 once, and start from it on every run."""
    key = "cp6_co"
    if key not in obj.data.attributes:
        a = obj.data.attributes.new(key, "FLOAT_VECTOR", "POINT")
        a.data.foreach_set("vector", coords(obj).astype(np.float32).ravel())
    v = np.empty(len(obj.data.vertices) * 3, dtype=np.float32)
    obj.data.attributes[key].data.foreach_get("vector", v)
    return v.reshape(-1, 3).astype(np.float64)


def widen(obj):
    c = keep_original(obj)
    t = np.clip((-0.16 - c[:, 2]) / 0.10, 0, 1)
    s = 1 + WIDEN * t * t * (3 - 2 * t)
    c[:, 0] *= s
    c[:, 1] = NECK[1] + (c[:, 1] - NECK[1]) * (1 + 0.35 * WIDEN * t)
    obj.data.vertices.foreach_set("co", c.astype(np.float32).ravel())
    obj.data.update()


def replace(name, me, mats):
    for m in mats:
        me.materials.append(m)
    old = bpy.data.objects.get(name)
    if old:
        om = old.data
        old.data = me
        bpy.data.meshes.remove(om)
        obj = old
    else:
        obj = bpy.data.objects.new(name, me)
        bpy.data.collections[COL].objects.link(obj)
    me.shade_smooth()
    return obj


def solidify(obj, thick, offset=1.0):
    m = obj.modifiers.get("Thickness") or obj.modifiers.new("Thickness", "SOLIDIFY")
    m.thickness = thick
    m.offset = offset
    m.use_even_offset = True
    s = obj.modifiers.get("Smooth") or obj.modifiers.new("Smooth", "SUBSURF")
    s.levels = 1
    s.render_levels = 2


def grid_object(name, rows, mats):
    """rows: list of lists of Vector, same length; quads between neighbouring rows."""
    bm = bmesh.new()
    vs = [[bm.verts.new(p) for p in r] for r in rows]
    for a, b in zip(vs[:-1], vs[1:]):
        for i in range(len(a) - 1):
            bm.faces.new((a[i], a[i + 1], b[i + 1], b[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name + "_mesh")
    bm.to_mesh(me)
    bm.free()
    return replace(name, me, mats)


def tube(name, pts, radius, mat):
    cu = bpy.data.curves.new(name + "_curve", "CURVE")
    cu.dimensions = "3D"
    sp = cu.splines.new("POLY")
    sp.points.add(len(pts) - 1)
    for p, q in zip(sp.points, pts):
        p.co = (q.x, q.y, q.z, 1)
    cu.bevel_depth = radius
    cu.bevel_resolution = 2
    tmp = bpy.data.objects.new("tmp_" + name, cu)
    bpy.data.collections[COL].objects.link(tmp)
    me = bpy.data.meshes.new_from_object(tmp.evaluated_get(bpy.context.evaluated_depsgraph_get()))
    bpy.data.objects.remove(tmp)
    bpy.data.curves.remove(cu)
    return replace(name, me, [mat])


def tree_of(names):
    bm = bmesh.new()
    for n in names:
        bm.from_mesh(bpy.data.objects[n].data)
    t = BVHTree.FromBMesh(bm)
    bm.free()
    return t


def v_half(z):
    """Half-width of the jacket's open front at height z (torso_form.jacket's V cut)."""
    zz = z + 0.045
    return min(max(0.060 * (zz + 0.430) / 0.200, 0.0), 0.075) + 0.012


# Same navy as the jacket, but without the jacket's painted colour layer (which only the
# jacket mesh has; on other meshes it would read as black).
NAVY = material("M_Bram_Garment_Navy", (0.030, 0.032, 0.095), 0.88)


def two_tone(obj, thick, outer, lining, offset):
    """Solidify with the outer face in one material and the inner face and edges in the
    lining material (Solidify's material offsets)."""
    me = obj.data
    me.materials.clear()
    me.materials.append(outer)
    me.materials.append(lining)
    m = obj.modifiers.get("Thickness") or obj.modifiers.new("Thickness", "SOLIDIFY")
    m.thickness = thick
    m.offset = offset
    m.use_even_offset = True
    m.material_offset = 1
    m.material_offset_rim = 1
    s = obj.modifiers.get("Smooth") or obj.modifiers.new("Smooth", "SUBSURF")
    s.levels = 1
    s.render_levels = 2


def collar(tree):
    """A plain raised collar round the neck, open at the front: navy outside, the mustard
    lining showing on its inner face and top edge. It stands from the jacket's neckline
    and flares out a little; higher at the back than at the front ends."""
    for old in ("Bram_Collar_Turned_Piping",):
        o = bpy.data.objects.get(old)
        if o:
            me = o.data
            bpy.data.objects.remove(o)
            bpy.data.meshes.remove(me)
    rows = []
    n = 41
    ths = [0.55 + (2 * math.pi - 1.10) * i / (n - 1) for i in range(n)]
    for k in range(5):
        f = k / 4
        row = []
        for th in ths:
            back = 0.5 - 0.5 * math.cos(th)
            z0 = -0.212 - 0.040 * back      # the base sinks into the jacket at the back
            z1 = -0.172 + 0.040 * back
            # the jacket's neck opening is wider at the back: the collar base meets it there
            r = 0.083 + 0.040 * back * back * (1 - 0.5 * f) + 0.010 * f * f
            d = Vector((1.12 * math.sin(th), -math.cos(th), 0.0))
            row.append(Vector((NECK[0], NECK[1], z0 + (z1 - z0) * f)) + d * r)
        rows.append(row)
    obj = grid_object("Bram_Collar_Turned", rows, [NAVY])
    # normals point out of the neck: offset -1 grows the thickness inwards, so the
    # outer face stays navy and the lining is the inside
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    c = Vector((NECK[0], NECK[1], -0.19))
    for face in bm.faces:
        out = face.calc_center_median() - c
        out.z = 0
        if face.normal.dot(out) < 0:
            face.normal_flip()
    bm.to_mesh(obj.data)
    bm.free()
    two_tone(obj, 0.0045, NAVY, MUSTARD, -1.0)
    return rows[-1]


def lapels(tree):
    """Each lapel lies on the jacket front beside the opening and rolls over at its inner
    edge; it runs from the collar point down to the bottom of the opening."""
    for sx, tag in ((-1, "R"), (1, "L")):
        rows = []
        nu, nv = 24, 9
        for iu in range(nu):
            u = iu / (nu - 1)
            z = -0.205 - 0.225 * u
            h = v_half(z)
            inner = h - 0.006
            width = 0.012 + 0.060 * math.sin(math.pi * min(1.0, 0.25 + u * 0.95)) * (1 - 0.65 * u)
            row = []
            for iv in range(nv):
                v = iv / (nv - 1)
                x = sx * (inner + width * v)
                hit, nrm, *_ = tree.ray_cast(Vector((x, -0.5, z)), Vector((0, 1, 0)))
                if hit is None:
                    hit, nrm = Vector((x, -0.10, z)), Vector((0, -1, 0))
                roll = 0.010 * (1 - v) ** 2 * (1 - 0.6 * u)   # the inner edge rolls up and over
                row.append(hit + nrm * (0.004 + roll))
            rows.append(row)
        obj = grid_object("Bram_Lapel_" + tag, rows, [NAVY])
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        for face in bm.faces:
            if face.normal.y > 0:      # face away from the body, towards the viewer
                face.normal_flip()
        bm.to_mesh(obj.data)
        bm.free()
        two_tone(obj, 0.0040, NAVY, MUSTARD, -1.0)
        tube("Bram_Lapel_Piping_" + tag, [r[-1] for r in rows], 0.0026, CORAL)


def shirt_points(tree):
    """Shirt collar points: from the front of the shirt's collar band, out and down over
    the shirt front. Placed from the band's own circle (the ray cast through the open
    neck would otherwise hit the back of the band)."""
    r = 0.074
    for sx, tag in ((-1, "R"), (1, "L")):
        rows = []
        for iu in range(8):
            u = iu / 7
            row = []
            for iv in range(6):
                v = iv / 5
                x = sx * (0.010 + 0.052 * u * (0.30 + 0.70 * v))
                z = -0.166 - 0.050 * u * (0.45 + 0.55 * (1 - abs(v - 0.6)))
                xn = min(abs(x) / 1.13, r * 0.98)
                y = NECK[1] - math.sqrt(r * r - xn * xn) - 0.004 - 0.010 * u
                hit, nrm, *_ = tree.ray_cast(Vector((x, -0.5, z)), Vector((0, 1, 0)))
                if hit is not None and hit.y < y + 0.004:
                    y = min(y, hit.y - 0.0025)
                row.append(Vector((x, y, z)))
            rows.append(row)
        obj = grid_object("Bram_Shirt_Collar_" + tag, rows, [CREAM])
        solidify(obj, 0.0020, 1.0)


def jacket_folds():
    s = sculpt_cp3.Sculpt("Bram_Jacket", None)
    s.co = s.base.copy()
    for sx in (-1, 1):
        # a pull from the shoulder towards the chest, a sag below the collar, a sleeve fold
        s.crease("shoulder pull", [(sx * 200, -60, -255), (sx * 150, -95, -310), (sx * 110, -110, -360)],
                 9.0, 2.2, lift_side=(sx, 0, 1), lift=1.8)
        s.crease("under lapel sag", [(sx * 150, -100, -380), (sx * 190, -95, -420)], 8.0, 1.6)
        s.crease("sleeve fold", [(sx * 245, -50, -380), (sx * 255, -40, -440)], 7.0, 1.8,
                 lift_side=(0, -1, 0), lift=1.2)
    s.commit()


def paint_cloth(lapel_pts):
    """Colour attribute on the jacket: plum where cloth meets cloth and in fold valleys,
    warm light on the upper planes of the shoulders. Read by the jacket materials."""
    obj = bpy.data.objects["Bram_Jacket"]
    me = obj.data
    c = coords(obj)
    n = len(c)
    col = np.ones((n, 4), dtype=np.float32)
    plum = np.array([0.40, 0.20, 0.36])
    warm = np.array([1.45, 1.20, 0.95])

    def mix(w, target):
        w = np.clip(w, 0, 1)[:, None]
        col[:, :3] = col[:, :3] * (1 - w) + target * w

    # joins: under the lapel's outer edge and round the collar base
    pts = np.array([[p.x, p.y, p.z] for p in lapel_pts])
    if len(pts):
        from mathutils.kdtree import KDTree
        kd = KDTree(len(pts))
        for i, p in enumerate(pts):
            kd.insert(p, i)
        kd.balance()
        d = np.array([kd.find(Vector(p))[2] for p in c])
        mix(np.exp(-(d / 0.010) ** 2) * 0.85, plum)
    dn = np.hypot(c[:, 0] / 1.12, c[:, 1] - NECK[1]) - 0.11
    mix(np.exp(-(dn / 0.015) ** 2) * (c[:, 2] > -0.27) * 0.7, plum)
    # warm light on the tops of the shoulders
    mix(np.clip((c[:, 2] + 0.27) / 0.05, 0, 1) * (np.abs(c[:, 0]) > 0.12) * 0.35, warm)
    attr = me.color_attributes.get("Paint_Cloth") or me.color_attributes.new("Paint_Cloth", "FLOAT_COLOR", "POINT")
    attr.data.foreach_set("color", col.ravel())
    for m in me.materials:
        nt = m.node_tree
        b = next(nd for nd in nt.nodes if nd.type == "BSDF_PRINCIPLED")
        if any(nd.type == "VERTEX_COLOR" for nd in nt.nodes):
            continue
        rgb = tuple(b.inputs["Base Color"].default_value)
        vc = nt.nodes.new("ShaderNodeVertexColor")
        vc.layer_name = "Paint_Cloth"
        mul = nt.nodes.new("ShaderNodeMix")
        mul.data_type = "RGBA"
        mul.blend_type = "MULTIPLY"
        mul.inputs["Factor"].default_value = 1.0
        mul.inputs[6].default_value = rgb
        nt.links.new(vc.outputs["Color"], mul.inputs[7])
        nt.links.new(mul.outputs[2], b.inputs["Base Color"])


def build():
    for name in ("Bram_Jacket", "Bram_Shirt"):
        widen(bpy.data.objects[name])
    for o in bpy.data.objects:
        if o.name.startswith(("Bram_Jacket_Piping", )):
            widen(o)
    # cp2 funnel collar and its piping, and the cp2 yoke piping (it cut through the new
    # collar at the back), are kept in the file but hidden
    for name in ("Bram_Collar", "Bram_Collar_Piping_L", "Bram_Collar_Piping_R",
                 "Bram_Jacket_Piping_YokeL", "Bram_Jacket_Piping_YokeR"):
        o = bpy.data.objects.get(name)
        if o:
            o.hide_render = True
            o.hide_viewport = True
    jacket_folds()
    tree = tree_of(["Bram_Jacket", "Bram_Shirt"])
    collar(tree_of(["Bram_Jacket"]))
    lapels(tree)
    shirt_points(tree_of(["Bram_Shirt"]))
    pts = []
    for tag in ("L", "R"):
        o = bpy.data.objects["Bram_Lapel_Piping_" + tag]
        pts += [Vector(v.co) for v in o.data.vertices][::6]
    paint_cloth(pts)
    return "clothes cp7 built"


print(build())
