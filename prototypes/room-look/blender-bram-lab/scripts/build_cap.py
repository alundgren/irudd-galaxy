# Bram's navy dockmaster cap: band fitted to the head mesh, a soft eight-panel crown,
# a peak, coral piping, a brass badge and a side button. Run inside Blender with the
# head object present. Edit the numbers below and run again; it replaces the cap parts.
import math

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

HEAD = "Bram_Head"
SEG = 160
BAND_FRONT_Z, BAND_BACK_Z = 0.047, 0.010   # lower edge of the band
BAND_H = 0.020
CENTER = Vector((0.0, 0.010, 0.0))
TILT_SIDE = 0.010   # the cap sits a little lower on his left (+X)
# Crown profile: (height above band top, radius factor, forward shift in metres)
# A soft, low crown that slouches forward over the brim
CROWN = [(0.000, 1.000, 0.000), (0.008, 1.075, 0.004), (0.018, 1.130, 0.010), (0.028, 1.150, 0.016),
         (0.037, 1.130, 0.021), (0.045, 1.070, 0.024), (0.052, 0.950, 0.026), (0.057, 0.780, 0.026),
         (0.061, 0.560, 0.025), (0.063, 0.340, 0.024), (0.0640, 0.140, 0.023), (0.0645, 0.0, 0.023)]
PANELS = 8
TOP_Z = 0.127   # just clears the top of the head; the slouch is in the forward shift
LEAN_X = 0.010   # the crown leans over to his left (+X)   # height of the crown's top centre; the crown rises to it from the band


def collection():
    col = bpy.data.collections["Bram_Asset"]
    return col


def material(name, rgb, rough, metal=0.0):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = (*rgb, 1)
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    return m


def replace_object(name, mesh):
    old = bpy.data.objects.get(name)
    if old:
        old_mesh = old.data
        old.data = mesh
        for m in old_mesh.materials:
            mesh.materials.append(m)
        bpy.data.meshes.remove(old_mesh)
        return old
    obj = bpy.data.objects.new(name, mesh)
    collection().objects.link(obj)
    return obj


def head_bvh():
    """The band sits on the head and on the hair volume where there is hair."""
    bm = bmesh.new()
    dg = bpy.context.evaluated_depsgraph_get()
    for name in (HEAD, "Bram_Hair_Mass"):
        obj = bpy.data.objects.get(name)
        if obj is not None:
            me = bpy.data.meshes.new_from_object(obj.evaluated_get(dg))
            me.transform(obj.matrix_world)
            bm.from_mesh(me)
            bpy.data.meshes.remove(me)
    tree = BVHTree.FromBMesh(bm)
    bm.free()
    return tree


def band_ring(tree, z_of, pad):
    pts = []
    for i in range(SEG):
        th = 2 * math.pi * i / SEG
        dirv = Vector((math.sin(th), -math.cos(th), 0.0))
        z = z_of(th)
        origin = Vector((CENTER.x, CENTER.y, z)) + dirv * 0.3
        hit = tree.ray_cast(origin, -dirv)
        r = (hit[0] - Vector((CENTER.x, CENTER.y, z))).length if hit[0] else 0.08
        pts.append((th, r + pad, z))
    return pts


def band_z(th, base_front, base_back):
    f = 0.5 + 0.5 * math.cos(th)  # 1 at the front, 0 at the back
    return base_back + (base_front - base_back) * f - TILT_SIDE * math.sin(th) * 0.5


def build():
    tree = head_bvh()
    lower = band_ring(tree, lambda th: band_z(th, BAND_FRONT_Z, BAND_BACK_Z), 0.0035)
    upper = band_ring(tree, lambda th: band_z(th, BAND_FRONT_Z + BAND_H, BAND_BACK_Z + BAND_H), 0.0045)
    # smooth the radii round the ring so the band is a clean fabric loop
    def smooth(pts, n=6):
        for _ in range(n):
            pts = [(pts[i][0], (pts[i - 1][1] + 2 * pts[i][1] + pts[(i + 1) % SEG][1]) / 4, pts[i][2])
                   for i in range(SEG)]
        return pts
    lower, upper = smooth(lower), smooth(upper)
    upper = [(th, max(r, lower[i][1] + 0.001), z) for i, (th, r, z) in enumerate(upper)]

    # Band
    bm = bmesh.new()
    rows = []
    for ring in (lower, upper):
        row = []
        for th, r, z in ring:
            row.append(bm.verts.new((CENTER.x + r * math.sin(th), CENTER.y - r * math.cos(th), z)))
        rows.append(row)
    for i in range(SEG):
        j = (i + 1) % SEG
        bm.faces.new((rows[0][i], rows[0][j], rows[1][j], rows[1][i]))
    band_mesh = bpy.data.meshes.new("Bram_Cap_Band_mesh")
    bm.to_mesh(band_mesh)
    bm.free()
    band = replace_object("Bram_Cap_Band", band_mesh)

    # Crown: rings above the band top, panels pulled in where they are sewn together, soft puff between
    bm = bmesh.new()
    rows = []
    n_rings = 40
    def profile(t):
        # piecewise-linear lookup in CROWN by normalised arc index
        x = t * (len(CROWN) - 1)
        k = min(int(x), len(CROWN) - 2)
        f = x - k
        a, b = CROWN[k], CROWN[k + 1]
        return tuple(a[i] + (b[i] - a[i]) * f for i in range(3))
    top_verts = None
    for k in range(n_rings):
        t = k / (n_rings - 1)
        dz, fac, fwd = profile(t)
        if fac <= 0.0:
            top_verts = bm.verts.new((CENTER.x + LEAN_X, CENTER.y - fwd, TOP_Z - 0.002))
            break
        row = []
        for i, (th, r, z) in enumerate(upper):
            stitch = abs(math.cos(PANELS / 2 * th))  # 1 on the stitched lines between panels (every 45 degrees)
            stitch_in = 0.0022 * stitch ** 24 * math.sin(math.pi * min(t * 1.2, 1.0))
            puff = 0.0025 * math.sin(math.pi * min(t * 1.6, 1.0)) * (1 - stitch ** 6)
            # the crown overhangs more at the front and sags a little on his left
            front = 0.5 + 0.5 * math.cos(th)
            # broad soft folds in the cloth, irregular round the crown, strongest mid-way up
            fold = 0.0032 * math.sin(5 * th + 0.7) * math.sin(3 * th + 2.1) * math.sin(math.pi * min(t * 1.25, 1.0))
            rr = r * fac * (1 + 0.10 * front * math.sin(math.pi * min(t * 1.3, 1.0))) + puff - stitch_in + fold
            zz = z + (dz / CROWN[-1][0]) * (TOP_Z - z) - 0.010 * math.sin(th) * t - 0.004 * front * t * t
            row.append(bm.verts.new((CENTER.x + LEAN_X * t + rr * math.sin(th), CENTER.y - fwd - rr * math.cos(th), zz)))
        rows.append(row)
    for a, b in zip(rows[:-1], rows[1:]):
        for i in range(SEG):
            j = (i + 1) % SEG
            bm.faces.new((a[i], a[j], b[j], b[i]))
    last = rows[-1]
    for i in range(SEG):
        bm.faces.new((last[i], last[(i + 1) % SEG], top_verts))
    crown_mesh = bpy.data.meshes.new("Bram_Cap_Crown_mesh")
    bm.normal_update()
    bm.to_mesh(crown_mesh)
    bm.free()
    crown = replace_object("Bram_Cap_Crown", crown_mesh)

    # Peak: from the lower front of the band, forward and down, slightly cupped
    bm = bmesh.new()
    half = 1.25
    cols = []
    idx = [i for i in range(SEG) if (lambda a: a <= half or a >= 2 * math.pi - half)(2 * math.pi * i / SEG)]
    idx = sorted(idx, key=lambda i: ((2 * math.pi * i / SEG + math.pi) % (2 * math.pi)) - math.pi)
    steps = 10
    for i in idx:
        th, r, z = lower[i]
        a = ((th + math.pi) % (2 * math.pi)) - math.pi
        u = abs(a) / half
        length = 0.068 * max(0.0, 1 - u ** 2.2) ** 0.55
        out = Vector((math.sin(th), -math.cos(th), 0.0))
        col_v = []
        for s in range(steps + 1):
            f = s / steps
            p = Vector((CENTER.x + r * out.x, CENTER.y + r * out.y, z + 0.001))
            p += out * (length * f)
            # curved brim: dips at its sides and a little at its front edge
            p.z -= length * f * 0.20 + 0.020 * (f ** 1.5) * (u ** 1.6) + 0.004 * f ** 3
            col_v.append(bm.verts.new(p))
        cols.append(col_v)
    for a, b in zip(cols[:-1], cols[1:]):
        for s in range(steps):
            bm.faces.new((a[s], b[s], b[s + 1], a[s + 1]))
    peak_edge = [c[-1].co.copy() for c in cols]
    peak_mesh = bpy.data.meshes.new("Bram_Cap_Peak_mesh")
    bm.to_mesh(peak_mesh)
    bm.free()
    peak = replace_object("Bram_Cap_Peak", peak_mesh)

    navy = material("M_Bram_Cap_Navy", (0.055, 0.060, 0.135), 0.85)
    coral = material("M_Bram_Coral", (0.62, 0.20, 0.16), 0.6)
    brass = material("M_Bram_Brass", (0.80, 0.55, 0.20), 0.38, 1.0)
    for obj, thick, mat in ((band, 0.0030, navy), (crown, 0.0030, navy), (peak, 0.0040, navy)):
        obj.data.materials.clear()
        obj.data.materials.append(mat)
        obj.data.shade_smooth()
        sol = obj.modifiers.get("Thickness") or obj.modifiers.new("Thickness", "SOLIDIFY")
        sol.thickness = thick
        sol.offset = 1.0 if obj is not peak else 0.0
        sol.use_even_offset = True
        if obj is not band:
            sub = obj.modifiers.get("Smooth") or obj.modifiers.new("Smooth", "SUBSURF")
            sub.levels = 1
            sub.render_levels = 1

    # Piping as tubes along the band edges and the peak rim
    def tube(name, pts, radius, cyclic):
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
        collection().objects.link(tmp)
        dg = bpy.context.evaluated_depsgraph_get()
        me = bpy.data.meshes.new_from_object(tmp.evaluated_get(dg))
        bpy.data.objects.remove(tmp)
        bpy.data.curves.remove(cu)
        obj = replace_object(name, me)
        obj.data.materials.clear()
        obj.data.materials.append(coral)
        obj.data.shade_smooth()
        return obj

    def ring_pts(ring, pad, dz=0.0):
        return [Vector((CENTER.x + (r + pad) * math.sin(th), CENTER.y - (r + pad) * math.cos(th), z + dz))
                for th, r, z in ring]
    tube("Bram_Cap_Piping_Top", ring_pts(upper, 0.0045, -0.0005), 0.0017, True)
    tube("Bram_Cap_Piping_Peak", peak_edge, 0.0016, False)

    # Brass badge on the forward-facing front of the crown, to his left (front right as seen)
    th = -0.10
    t_badge = 0.10
    dz, fac, fwd = profile(t_badge)
    i = int(th / (2 * math.pi) * SEG)
    _, r, z = upper[i]
    origin = Vector((CENTER.x, CENTER.y - fwd, z + (dz / CROWN[-1][0]) * (TOP_Z - z)))
    outdir = Vector((math.sin(th), -math.cos(th), 0.05)).normalized()
    hit, normal, *_ = BVHTree.FromObject(crown, bpy.context.evaluated_depsgraph_get()).ray_cast(origin + outdir * 0.3, -outdir)
    if hit is None:
        hit, normal = origin + outdir * r * fac, outdir
    bm = bmesh.new()
    w0, w1, h = 0.044, 0.034, 0.034
    outline = [(-w0 / 2, -h / 2), (w0 / 2, -h / 2), (w1 / 2 + 0.002, h / 2), (0.003, h / 2),
               (0.0, h / 2 - 0.004), (-0.003, h / 2), (-w1 / 2 - 0.002, h / 2)]
    face = bm.faces.new([bm.verts.new((x, y, 0.0)) for x, y in outline])
    ext = bmesh.ops.extrude_face_region(bm, geom=[face])
    for v in ext["geom"]:
        if isinstance(v, bmesh.types.BMVert):
            v.co.z += 0.0025
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    badge_mesh = bpy.data.meshes.new("Bram_Cap_Badge_mesh")
    bm.to_mesh(badge_mesh)
    bm.free()
    badge = replace_object("Bram_Cap_Badge", badge_mesh)
    up = Vector((0, 0, 1))
    zax = normal.normalized()
    xax = up.cross(zax).normalized() * -1
    yax = zax.cross(xax)
    roll = math.radians(-9)
    xax, yax = xax * math.cos(roll) + yax * math.sin(roll), yax * math.cos(roll) - xax * math.sin(roll)
    badge.matrix_world = Matrix.Translation(hit - zax * 0.0005) @ Matrix((
        (xax.x, yax.x, zax.x, 0), (xax.y, yax.y, zax.y, 0), (xax.z, yax.z, zax.z, 0), (0, 0, 0, 1)))
    badge.data.materials.clear()
    badge.data.materials.append(brass)
    bev = badge.modifiers.get("Bevel") or badge.modifiers.new("Bevel", "BEVEL")
    bev.width = 0.0009
    bev.segments = 2

    # Side button on the band, his right side (-X), with a coral ring
    th = -1.30 % (2 * math.pi)
    i = int(th / (2 * math.pi) * SEG)
    _, r, z = upper[i]
    zc = (z + lower[i][2]) / 2 + 0.004
    out = Vector((math.sin(th), -math.cos(th), 0.0))
    pos = Vector((CENTER.x + (r + 0.002) * out.x, CENTER.y + (r + 0.002) * out.y, zc))
    for name, rad, depth, mat, push in (("Bram_Cap_Button_Ring", 0.0085, 0.0025, coral, 0.0),
                                        ("Bram_Cap_Button", 0.0062, 0.0042, brass, 0.0012)):
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=32, radius1=rad, radius2=rad * 0.92, depth=depth)
        me = bpy.data.meshes.new(name + "_mesh")
        bm.to_mesh(me)
        bm.free()
        obj = replace_object(name, me)
        rot = out.to_track_quat("Z", "Y").to_matrix().to_4x4()
        obj.matrix_world = Matrix.Translation(pos + out * (depth / 2 + push)) @ rot
        obj.data.materials.clear()
        obj.data.materials.append(mat)
        b = obj.modifiers.get("Bevel") or obj.modifiers.new("Bevel", "BEVEL")
        b.width = 0.0008
        b.segments = 2
        obj.data.shade_smooth()
    return "cap built"


print(build())
