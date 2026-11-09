# Bram's hair, brows and beard as connected volumes with a few deliberate strands.
# - Hair: a dark volume over the scalp (metaball elements sitting on the head surface),
#   with irregular curl clumps of different sizes on top. The temples and sideburns are
#   grey; a few dozen separate grey locks sweep back and down over the dark hair.
# - Brows: a tapered mass along each arch (his right arch higher), with short hairs
#   breaking up the edges.
# - Beard: a short connected volume over chin and jaw, thick at the chin and thin at the
#   edges, coloured dark to grey; a trimmed moustache; a few grey hairs at the edges.
# Run inside Blender with Bram_Head present. Seeded, so a rerun gives the same hair.
import math
import random

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

import paint_utils

HEAD = "Bram_Head"
SEED = 23
FACE_W = 1.05


def material(name, rgb, rough):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = (*rgb, 1)
    b.inputs["Roughness"].default_value = rough
    return m


HAIR_DARK = material("M_Bram_Hair_Dark", (0.022, 0.019, 0.018), 0.62)
HAIR_GREY = material("M_Bram_Hair_Grey", (0.15, 0.145, 0.14), 0.72)
HAIR_SILVER = material("M_Bram_Hair_Silver", (0.34, 0.33, 0.32), 0.70)


def head_data():
    # the head's evaluated surface (with its Subdivision modifier), not the control mesh
    obj = bpy.data.objects[HEAD]
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(obj.evaluated_get(dg))
    me.transform(obj.matrix_world)
    n = len(me.vertices)
    co = np.empty(n * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    no = np.empty(n * 3, dtype=np.float32)
    me.vertices.foreach_get("normal", no)
    bm = bmesh.new()
    bm.from_mesh(me)
    tree = BVHTree.FromBMesh(bm)
    bm.free()
    global EDGES
    EDGES = np.empty(len(me.edges) * 2, dtype=np.int32)
    me.edges.foreach_get("vertices", EDGES)
    EDGES = EDGES.reshape(-1, 2)
    global TRIS
    me.calc_loop_triangles()
    TRIS = np.empty(len(me.loop_triangles) * 3, dtype=np.int32)
    me.loop_triangles.foreach_get("vertices", TRIS)
    TRIS = TRIS.reshape(-1, 3)
    bpy.data.meshes.remove(me)
    return co.reshape(-1, 3), no.reshape(-1, 3), tree


def soften(mask, iterations):
    """Blur a 0/1 vertex mask over the head mesh, giving 0..1 with soft edges."""
    v = mask.astype(np.float32)
    n = len(v)
    deg = np.bincount(EDGES.ravel(), minlength=n).astype(np.float32) + 1e-6
    for _ in range(iterations):
        acc = np.zeros(n, dtype=np.float32)
        np.add.at(acc, EDGES[:, 0], v[EDGES[:, 1]])
        np.add.at(acc, EDGES[:, 1], v[EDGES[:, 0]])
        v = 0.5 * v + 0.5 * acc / deg
    return v


def spread(co, mask, spacing, rng):
    """Pick masked points at least `spacing` apart (grid-hashed dart throwing)."""
    idx = np.flatnonzero(mask)
    rng.shuffle(idx)
    cells = {}
    out = []
    s2 = spacing * spacing
    for i in idx:
        p = co[i]
        k = tuple((p // spacing).astype(int))
        near = False
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                for dz in (-1, 0, 1):
                    for j in cells.get((k[0] + dx, k[1] + dy, k[2] + dz), ()):
                        if ((co[j] - p) ** 2).sum() < s2:
                            near = True
                            break
                    if near:
                        break
                if near:
                    break
            if near:
                break
        if not near:
            cells.setdefault(k, []).append(i)
            out.append(i)
    return out


def replace(name, me, mats):
    # Clearing material slots resets every face's material index, so only clear a mesh
    # that already has slots; new meshes built from bmesh keep their indices.
    if len(me.materials):
        me.materials.clear()
    for m in mats:
        me.materials.append(m)
    me.shade_smooth()
    old = bpy.data.objects.get(name)
    if old:
        om = old.data
        old.data = me
        bpy.data.meshes.remove(om)
        return old
    obj = bpy.data.objects.new(name, me)
    bpy.data.collections["Bram_Asset"].objects.link(obj)
    return obj


def metaball_mesh(name, elements, resolution, threshold, mats):
    """elements: list of (centre, radius, (sx, sy, sz) ellipsoid scale, rotation quaternion).
    Metaball resolution has a lower limit far above a millimetre, so the elements are built
    ten times larger and the resulting mesh is scaled back down."""
    up = 10.0
    mb = bpy.data.metaballs.new(name + "_mb")
    mb.resolution = resolution * up
    mb.render_resolution = resolution * up
    mb.threshold = threshold
    for c, r, scale, rot in elements:
        e = mb.elements.new(type="ELLIPSOID")
        e.co = Vector(c) * up
        e.radius = r * up
        e.size_x, e.size_y, e.size_z = scale
        e.rotation = rot
    tmp = bpy.data.objects.new("tmp_" + name, mb)
    bpy.context.scene.collection.objects.link(tmp)
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(tmp.evaluated_get(dg))
    me.transform(Matrix.Scale(1.0 / up, 4))
    bpy.data.objects.remove(tmp)
    bpy.data.metaballs.remove(mb)
    return replace(name, me, mats)


def add_tube(bm, pts, radii, sides, mat_index, flat=1.0):
    n = len(pts)
    rings = []
    prev = None
    for i in range(n):
        t = (pts[min(i + 1, n - 1)] - pts[max(i - 1, 0)]).normalized()
        ref = prev if prev is not None else (Vector((0, 0, 1)) if abs(t.z) < 0.9 else Vector((1, 0, 0)))
        nrm = (ref - t * ref.dot(t)).normalized()
        prev = nrm
        bi = t.cross(nrm)
        rings.append([bm.verts.new(pts[i] + nrm * math.cos(2 * math.pi * k / sides) * radii[i] * flat
                                   + bi * math.sin(2 * math.pi * k / sides) * radii[i]) for k in range(sides)])
    faces = []
    for a, b in zip(rings[:-1], rings[1:]):
        for k in range(sides):
            faces.append(bm.faces.new((a[k], a[(k + 1) % sides], b[(k + 1) % sides], b[k])))
    tip = bm.verts.new(pts[-1] + (pts[-1] - pts[-2]).normalized() * radii[-1])
    for k in range(sides):
        faces.append(bm.faces.new((rings[-1][k], rings[-1][(k + 1) % sides], tip)))
    faces.append(bm.faces.new(list(reversed(rings[0]))))
    for f in faces:
        f.material_index = mat_index
        f.smooth = True


def tubes_object(name, build, mats):
    bm = bmesh.new()
    count = build(bm)
    me = bpy.data.meshes.new(name + "_mesh")
    bm.to_mesh(me)
    bm.free()
    return replace(name, me, mats), count


def hairline(co):
    x, y, z = co[:, 0] / FACE_W, co[:, 1], co[:, 2]
    a = np.abs(np.arctan2(x, -(y - 0.010)))
    low = np.interp(a, [0.0, 0.62, 0.80, 0.95, 1.20, 1.36, 1.55, 1.95, 2.25, 3.2],
                    [0.075, 0.050, 0.030, 0.012, -0.004, -0.028, 0.012, 0.014, -0.045, -0.062])
    ear = (np.abs(np.abs(co[:, 0]) - 0.077) < 0.020) & (np.abs(y - 0.013) < 0.023) & (z < 0.017) & (z > -0.052)
    return a, low, ear


def curl_group(p, n, rng, length, size):
    """A directed curl group: a continuous chain of overlapping clumps sweeping back and
    down over the scalp with a slight sideways turn, tapering towards its end."""
    flow = Vector((0.0, 0.55, -0.85))
    flow = (flow - n * flow.dot(n)).normalized()
    side = n.cross(flow).normalized()
    turn = rng.uniform(-1.0, 1.0)
    out = []
    steps = 8
    for k in range(steps):
        f = k / (steps - 1)
        c = (p + n * (0.0028 + 0.0014 * math.sin(math.pi * f)) + flow * length * f
             + side * 0.0030 * turn * math.sin(2.4 * f))
        out.append((c, size * (1.0 - 0.35 * f), (1.0, 0.85, 0.75), (flow + n * 0.3).normalized().to_track_quat("Z", "Y")))
    return out


def hair(co, no, rng, prng):
    """One hair volume: a dark layer over the scalp and directed curl groups on it. Each
    group is dark, grey or silver as a whole (grey more likely at the temples and sides,
    rarer at the back and crown); the faces of the volume take the colour of the nearest
    curl group, or dark where only the under-layer is."""
    from mathutils.kdtree import KDTree
    a, low, ear = hairline(co)
    z = co[:, 2]
    # stop just above the cap band (same band heights as build_cap.py), so no curl
    # pokes through the low crown
    phi = np.arctan2(co[:, 0], -(co[:, 1] - 0.010))
    band_top = 0.010 + 0.037 * (0.5 + 0.5 * np.cos(phi)) - 0.005 * np.sin(phi) + 0.020
    region = (z > low) & (a > 0.55) & ~ear & (z < band_top)
    elements, labels = [], []
    for i in spread(co, region, 0.0075, prng):
        p, n = Vector(co[i]), Vector(no[i])
        elements.append((p + n * 0.0010, 0.0105, (1.0, 1.0, 0.42), n.to_track_quat("Z", "Y")))
        labels.append(-1)
    groups = 0
    for i in spread(co, region & (z > low + 0.003), 0.0100, prng):
        p, n = Vector(co[i]), Vector(no[i])
        temple = max(0.0, min(1.0, (2.3 - float(a[i])) / 1.2))
        r = rng.random()
        if r < 0.06 + 0.52 * temple ** 1.5:
            shade = 2 if rng.random() < 0.15 else 1
        else:
            shade = 0
        for e in curl_group(p, n, rng, rng.uniform(0.009, 0.013), rng.uniform(0.0050, 0.0066)):
            elements.append(e)
            labels.append(shade)
        groups += 1
    mats = [HAIR_DARK, material("M_Bram_Hair_Lock_Grey", (0.22, 0.21, 0.20), 0.72),
            material("M_Bram_Hair_Lock_Silver", (0.42, 0.41, 0.39), 0.70)]
    mass = metaball_mesh("Bram_Hair_Mass", elements, 0.0009, 0.6, mats)
    kd = KDTree(len(elements))
    for k, e in enumerate(elements):
        kd.insert(e[0], k)
    kd.balance()
    me = mass.data
    fc = np.empty(len(me.polygons) * 3, dtype=np.float32)
    me.polygons.foreach_get("center", fc)
    idx = []
    for c in fc.reshape(-1, 3):
        # the nearest curl element decides; the under-layer only counts if nothing else is close
        best = [lab for _, k, d in kd.find_n(Vector(c), 3) if labels[k] >= 0 for lab in [labels[k]]]
        idx.append(best[0] if best else 0)
    me.polygons.foreach_set("material_index", np.array(idx, dtype=np.int32))
    me.update()
    old = bpy.data.objects.get("Bram_Hair_Locks")
    if old:
        om = old.data
        bpy.data.objects.remove(old)
        bpy.data.meshes.remove(om)
    return mass, groups


def grey_locks(co, no, rng, prng):
    """Kept for the call order: the grey groups are part of the hair volume (hair())."""
    return None, 0


def brow_path(sx, f):
    """Point on the brow arch, front coordinates; f runs from the inner (0) to outer (1) end."""
    arch = 0.0048 if sx < 0 else 0.0010   # his right brow raised in a round arch
    x = sx * (0.011 + 0.047 * f) * FACE_W
    # inner ends lifted a little: attentive and friendly rather than a scowl
    z = 0.0155 + 0.0030 * (1 - f) ** 2 + (0.0050 + arch) * math.sin(math.pi * min(1.0, f * 1.18)) - 0.007 * f ** 2
    if sx > 0:
        z -= 0.0015 * f  # his left brow sits a little lower and flatter
    return x, z


def brows(tree, rng):
    elements = []
    for sx in (-1, 1):
        for k in range(26):
            f = k / 25
            x, z = brow_path(sx, f)
            hit, nrm, *_ = tree.ray_cast(Vector((x, -0.2, z)), Vector((0, 1, 0)))
            if hit is None:
                continue
            # broad, blunt inner end; a long flat taper to the outer tail
            width = 0.0046 * (1.0 - 0.62 * f ** 1.4) if f > 0.06 else 0.0038
            thick = 0.0011 * (1.0 - 0.45 * f)
            elements.append((hit - nrm * 0.0002, 0.0060,
                             (width / 0.006 * 1.25, width / 0.006, thick / 0.006 * 1.6), nrm.to_track_quat("Z", "Y")))

    def build(bm):
        count = 0
        for sx in (-1, 1):
            for k in range(110):
                f = rng.random()
                x, z = brow_path(sx, f)
                z += rng.uniform(-0.0035, 0.0035) * (1 - 0.5 * f)
                hit, nrm, *_ = tree.ray_cast(Vector((x, -0.2, z)), Vector((0, 1, 0)))
                if hit is None:
                    continue
                ang = math.radians(70 - 95 * f + rng.uniform(-12, 12))
                d = Vector((sx * math.cos(ang), 0.0, math.sin(ang)))
                d = (d - nrm * d.dot(nrm)).normalized()
                ln = rng.uniform(0.0045, 0.0080)
                pts = [hit + nrm * (0.0006 + 0.0006 * math.sin(math.pi * s / 4)) + d * ln * s / 4 for s in range(5)]
                add_tube(bm, pts, [0.00050 * (1 - 0.7 * s / 4) for s in range(5)], 4, 0, 0.6)
                count += 1
        return count
    brow_mat = material("M_Bram_Brow", (0.016, 0.013, 0.012), 0.92)
    tubes_object("Bram_Brow_Hairs", build, [brow_mat])
    return metaball_mesh("Bram_Brows", elements, 0.0006, 0.6, [brow_mat]), len(elements)


def surface_points(region_w, density_per_cm2, rng):
    """Random points on the head surface, area-weighted, kept with probability region_w
    (interpolated over each triangle); returns positions and normals."""
    tri = TRIS
    a, b, c = CO[tri[:, 0]], CO[tri[:, 1]], CO[tri[:, 2]]
    area = 0.5 * np.linalg.norm(np.cross(b - a, c - a), axis=1)
    w = region_w[tri].mean(axis=1)
    sel = w > 0.02
    expected = area[sel] * 1e4 * density_per_cm2 * w[sel]
    counts = rng.poisson(expected)
    ti = np.repeat(np.flatnonzero(sel), counts)
    r1, r2 = rng.random(len(ti)), rng.random(len(ti))
    flip = r1 + r2 > 1
    r1[flip], r2[flip] = 1 - r1[flip], 1 - r2[flip]
    pa, pb, pc = CO[tri[ti, 0]], CO[tri[ti, 1]], CO[tri[ti, 2]]
    pts = pa + (pb - pa) * r1[:, None] + (pc - pa) * r2[:, None]
    nrm = NO[tri[ti, 0]] + NO[tri[ti, 1]] + NO[tri[ti, 2]]
    nrm /= np.linalg.norm(nrm, axis=1, keepdims=True) + 1e-9
    return pts, nrm, ti


def short_hairs(name, pts, nrm, rng, length, radius, mix, down, mats):
    """One short tapered hair per point, leaning down with gravity; colour chosen from
    mix (probabilities for the materials, in order)."""
    def build(bm):
        for p, n in zip(pts, nrm):
            p, n = Vector(p), Vector(n)
            d = (n * 0.55 + Vector((rng.uniform(-0.35, 0.35), rng.uniform(-0.1, 0.2), -down))).normalized()
            ln = length * rng.uniform(0.6, 1.2)
            bend = Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))) * 0.25 * ln
            q = [p + d * ln * f + bend * f * f for f in (0.0, 0.35, 0.7, 1.0)]
            r = rng.random()
            mi = 0 if r < mix[0] else (1 if r < mix[0] + mix[1] else 2)
            add_tube(bm, q, [radius, radius * 0.8, radius * 0.55, radius * 0.3], 4, mi, 0.8)
        return len(pts)
    return tubes_object(name, build, mats)


def beard(co, no, tree, rng, prng):
    """Short salt-and-pepper facial hair: separate short hairs with skin showing between
    them, densest on the chin, thinner along the jaw, a moustache that follows the lip;
    the upper edge of the jaw hair is broken rather than ruled."""
    x, y, z = co[:, 0] / FACE_W, co[:, 1], co[:, 2]
    front = y < -0.035
    lip = (y < -0.092) & (z > -0.096) & (z < -0.062)
    chin = front & (z < -0.088) & (np.abs(x) < 0.030) & (y < -0.055) & (z > -0.132) & ~lip
    top = -0.083 - 0.45 * np.maximum(np.abs(x) - 0.024, 0.0)
    jaw = (z < top) & (np.abs(x) < 0.058) & (y < -0.005) & (z > -0.128) & ~lip
    stache = (z > -0.0635) & (z < -0.0525) & (np.abs(x) < 0.025) & (y < -0.085)
    soft = lambda m, k: np.clip(soften(m, k), 0, 1)
    g = np.asarray(paint_utils.grain(co, 900.0), dtype=np.float64)
    # broken edge: the fade is pushed up and down by a coarse grain before thresholding
    near_jaw = soft(jaw | chin, 10)
    jaw_w = np.clip((near_jaw - 0.25 + 0.5 * (g - 0.5)) / 0.5, 0, 1) * (near_jaw > 0.08)
    chin_w = soft(chin, 8)
    near_stache = soft(stache, 5)
    stache_w = np.clip((near_stache - 0.15 + 0.4 * (g - 0.5)) / 0.5, 0, 1) * (near_stache > 0.08)
    mats = [material("M_Bram_Beard_Dark", (0.025, 0.022, 0.020), 0.7),
            material("M_Bram_Beard_Grey", (0.26, 0.25, 0.24), 0.75),
            material("M_Bram_Beard_Silver", (0.50, 0.49, 0.47), 0.7)]
    nrng = np.random.default_rng(SEED + 5)
    p1, n1, _ = surface_points(np.maximum(chin_w, 0.0), 105.0, nrng)
    p2, n2, _ = surface_points(np.clip(jaw_w - chin_w, 0, 1), 42.0, nrng)
    pts = np.concatenate([p1, p2])
    nrm = np.concatenate([n1, n2])
    obj, n = short_hairs("Bram_Beard", p1, n1, rng, 0.0050, 0.00030, (0.40, 0.42, 0.18), 0.55, mats)
    jawobj, n2c = short_hairs("Bram_Beard_Jaw", p2, n2, rng, 0.0026, 0.00024, (0.68, 0.27, 0.05), 0.45, mats)
    p3, n3, _ = surface_points(stache_w, 60.0, nrng)
    so, n3c = short_hairs("Bram_Moustache", p3, n3, rng, 0.0034, 0.00026, (0.62, 0.33, 0.05), 0.95, mats)
    # one beard object: join the jaw hairs into it
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.from_mesh(jawobj.data)
    bm.to_mesh(obj.data)
    bm.free()
    jm = jawobj.data
    bpy.data.objects.remove(jawobj)
    bpy.data.meshes.remove(jm)
    return obj, n + n2c + n3c


for old in ("Bram_Hair_Curls", "Bram_Beard_Hairs"):
    o = bpy.data.objects.get(old)
    if o:
        me = o.data
        bpy.data.objects.remove(o)
        bpy.data.meshes.remove(me)

CO, NO, TREE = head_data()
CO = CO.astype(np.float64)
NO = NO.astype(np.float64)
RNG = random.Random(SEED)
PRNG = np.random.default_rng(SEED)
print("curl groups", hair(CO, NO, RNG, PRNG)[1])
print("grey locks", grey_locks(CO, NO, RNG, PRNG)[1])
print("brow elements", brows(TREE, RNG)[1])
print("beard elements", beard(CO, NO, TREE, RNG, PRNG)[1])
