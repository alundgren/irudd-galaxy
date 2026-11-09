# Measure facial landmarks on Bram_Head (with its active shape keys) so proportions can be
# compared between checkpoints as numbers. Coordinates in mm; -Y is forward, +Z up.
# The midline profile is the most forward surface point in each 0.5 mm height band.
import bpy
import numpy as np


def head_coords(name="Bram_Head"):
    obj = bpy.data.objects[name]
    dg = bpy.context.evaluated_depsgraph_get()
    me = obj.evaluated_get(dg).to_mesh()
    co = np.empty(len(me.vertices) * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    obj.evaluated_get(dg).to_mesh_clear()
    co = co.reshape(-1, 3)
    m = np.array(obj.matrix_world)
    co = co @ m[:3, :3].T + m[:3, 3]
    return co * 1000.0


def profile(co, half_width=1.0):
    """Midline profile: for each 0.5 mm height, the most forward surface point near x = 0.
    Uses a ray cast on the evaluated mesh when one is available (coarse meshes have too few
    vertices on the midline), else the vertices themselves."""
    zs = np.arange(-135.0, 40.0, 0.5)
    out = []
    if _TREE is not None:
        from mathutils import Vector
        for z in zs:
            hit = _TREE.ray_cast(Vector((0.0, -400.0, z)), Vector((0, 1, 0)))
            out.append((z, hit[0].y if hit[0] is not None and hit[0].y < -40 else np.nan))
        return np.array(out)
    mid = co[(np.abs(co[:, 0]) < half_width) & (co[:, 1] < -40)]
    for z in zs:
        band = mid[np.abs(mid[:, 2] - z) < 0.5]
        out.append((z, band[:, 1].min()) if len(band) else (z, np.nan))
    return np.array(out)


_TREE = None


def use_tree_of(name):
    """Measure the named object's evaluated surface by ray casts (world space, mm)."""
    global _TREE
    from mathutils import Matrix
    from mathutils.bvhtree import BVHTree
    obj = bpy.data.objects[name]
    dg = bpy.context.evaluated_depsgraph_get()
    me = obj.evaluated_get(dg).to_mesh()
    import bmesh
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.transform(Matrix.Scale(1000.0, 4) @ obj.matrix_world)
    _TREE = BVHTree.FromBMesh(bm)
    bm.free()
    obj.evaluated_get(dg).to_mesh_clear()


def window(prof, z0, z1, pick):
    sel = prof[(prof[:, 0] >= min(z0, z1)) & (prof[:, 0] <= max(z0, z1)) & ~np.isnan(prof[:, 1])]
    i = np.argmin(sel[:, 1]) if pick == "front" else np.argmax(sel[:, 1])
    return sel[i]


def measure(co=None):
    co = head_coords() if co is None else co
    prof = profile(co)
    L = {}
    L["glabella"] = window(prof, 5, 30, "front")
    L["nasion"] = window(prof, -8, L["glabella"][0] - 2, "back")
    L["pronasale"] = window(prof, -60, -15, "front")
    pz = L["pronasale"][0]
    L["subnasale"] = window(prof, pz - 22, pz - 5, "back")
    sz = L["subnasale"][0]
    L["labrale_sup"] = window(prof, sz - 22, sz - 6, "front")
    L["stomion"] = window(prof, L["labrale_sup"][0] - 12, L["labrale_sup"][0] - 1, "back")
    L["labrale_inf"] = window(prof, L["stomion"][0] - 16, L["stomion"][0] - 2, "front")
    L["sublabiale"] = window(prof, L["labrale_inf"][0] - 20, L["labrale_inf"][0] - 3, "back")
    L["pogonion"] = window(prof, -128, L["sublabiale"][0] - 3, "front")
    chin = co[(np.abs(co[:, 0]) < 3) & (co[:, 1] < L["pogonion"][1] + 25) & (co[:, 2] > -140) & (co[:, 2] < L["pogonion"][0])]
    L["menton"] = np.array([chin[:, 2].min(), chin[np.argmin(chin[:, 2]), 1]])
    if _TREE is not None:
        from mathutils import Vector
        best = None
        for y in np.arange(L["pogonion"][1], L["pogonion"][1] + 22, 0.5):
            hit = _TREE.ray_cast(Vector((0.0, float(y), -400.0)), Vector((0, 0, 1)))
            if hit[0] is not None and hit[0].z < L["pogonion"][0] and (best is None or hit[0].z < best[0]):
                best = (hit[0].z, y)
        if best is not None:
            L["menton"] = np.array(best)
    # widths
    row = co[(np.abs(co[:, 2] - (L["subnasale"][0] + 4)) < 0.6) & (co[:, 1] < L["subnasale"][1] + 4)]
    alar = np.abs(row[:, 0]).max() * 2 if len(row) else np.nan
    zyg = co[(co[:, 2] > -35) & (co[:, 2] < 0) & (co[:, 1] < -20)]
    bizyg = np.abs(zyg[:, 0]).max() * 2
    cheek = []
    for sx in (-1, 1):
        c = co[(np.abs(co[:, 0] - sx * 40) < 1.5) & (np.abs(co[:, 2] + 30) < 1.5) & (co[:, 1] < -40)]
        cheek.append(c[:, 1].min())
    n, sn, sto, me = L["nasion"][0], L["subnasale"][0], L["stomion"][0], L["menton"][0]
    prn, pog = L["pronasale"], L["pogonion"]

    def eline(p):
        # signed distance (mm) of profile point p from the line pronasale-pogonion, + = in front
        d = np.array([pog[1] - prn[1], pog[0] - prn[0]])
        q = np.array([p[1] - prn[1], p[0] - prn[0]])
        return -(d[0] * q[1] - d[1] * q[0]) / np.linalg.norm(d) * np.sign(d[1])
    R = {
        "face height nasion-menton": n - me,
        "nose length nasion-subnasale": n - sn,
        "upper lip subnasale-stomion": sn - sto,
        "lower face stomion-menton": sto - me,
        "nose / face height": (n - sn) / (n - me),
        "upper lip / face height": (sn - sto) / (n - me),
        "stomion-menton / face height": (sto - me) / (n - me),
        "nose tip ahead of subnasale": L["subnasale"][1] - prn[1],
        "nose tip drop below its bridge line": 0.0,
        "upper lip vs nose-chin line": -eline(L["labrale_sup"]),
        "lower lip vs nose-chin line": -eline(L["labrale_inf"]),
        "lip ahead of subnasale": L["subnasale"][1] - L["labrale_sup"][1],
        "alar width": alar,
        "face width at cheekbones": bizyg,
        "cheek front at x=40,z=-30 vs subnasale (R, L)": (cheek[0] - L["subnasale"][1], cheek[1] - L["subnasale"][1]),
    }
    del R["nose tip drop below its bridge line"]
    return L, R


def report(tag):
    L, R = measure()
    lines = [f"[{tag}]"]
    for k, v in L.items():
        lines.append(f"  {k:12s} z={v[0]:7.1f}  y={v[1]:7.1f}")
    for k, v in R.items():
        if isinstance(v, tuple):
            lines.append(f"  {k}: {v[0]:.1f}, {v[1]:.1f}")
        else:
            lines.append(f"  {k}: {v:.2f}")
    return "\n".join(lines)
