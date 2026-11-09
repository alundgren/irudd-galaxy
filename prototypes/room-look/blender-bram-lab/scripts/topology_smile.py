# Face diagnostic after cp7: a half smile built on the base mesh's own topology instead of
# distance-based regions. It uses the authored face sets of the CC0 head (7 mouth,
# 9/10 eye areas with the lids, 5/6 eye sockets, 11 nose, 24 lower face, 33 head).
#
# 1. Find the mouth corners: the lip vertices where the outer surface turns into the
#    mouth (front vertices of face set 7 with an edge into the mouth interior), taking the
#    outermost one on each side.
# 2. Find each lower-lid margin: lid vertices (face sets 9/10) that share an edge with
#    the socket (5/6) and lie below the eye centre.
# 3. Grow regions over edges (rings counted in edge steps), so each region follows the
#    loops round the mouth and eye, and move them together:
#    - corner rings: up, back and out (more on his left, +X);
#    - cheek: rings grown from the vertices just outside the corner and below the lid,
#      not into the lips, lids or nose; lifted and pushed forward so it bunches;
#    - lower lid: margin and the next rings up, so the lid compresses from below;
#    - lips: pushed back along their normals, more in the middle, to relax the pucker.
# 4. One masked Sculpt Mode smooth over the edited rings only.
import bmesh
import bpy
import numpy as np
from collections import deque

HEAD = "Bram_Head"
LEFT = 1   # his left, +X: the smiling side
CHEEK = 1.0  # extra scale for the cheek lift, independent of the lips


def rings_from(bm, seeds, max_ring, allowed):
    """Edge-step distance from the seed vertices, walking only through allowed vertices."""
    dist = {v: 0 for v in seeds}
    q = deque(seeds)
    while q:
        v = q.popleft()
        if dist[v] >= max_ring:
            continue
        for e in bm.verts[v].link_edges:
            o = e.other_vert(bm.verts[v]).index
            if o not in dist and allowed[o]:
                dist[o] = dist[v] + 1
                q.append(o)
    return dist


def falloff(dist, max_ring):
    w = {}
    for v, d in dist.items():
        t = d / (max_ring + 1)
        w[v] = 0.5 + 0.5 * np.cos(np.pi * t)
    return w


def run(strength=1.0, report=print):
    obj = bpy.data.objects[HEAD]
    me = obj.data
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.verts.ensure_lookup_table()
    bm.normal_update()
    n = len(bm.verts)
    co = np.array([v.co[:] for v in bm.verts]) * 1000.0
    nrm = np.array([v.normal[:] for v in bm.verts])
    fs_poly = np.empty(len(me.polygons), dtype=np.int32)
    me.attributes[".sculpt_face_set"].data.foreach_get("value", fs_poly)
    vfs = [set() for _ in range(n)]
    for f in bm.faces:
        for v in f.verts:
            vfs[v.index].add(int(fs_poly[f.index]))
    in_set = lambda i, s: bool(vfs[i] & s)
    front = co[:, 1] < -60

    # 1. mouth corners
    # Face set 7 is the inner mouth bag. Where it meets the outer face (24/33) is the
    # line where the lips meet; its outermost vertex on each side is the mouth corner.
    lip_line = [i for i in range(n) if in_set(i, {7}) and in_set(i, {24, 33}) and co[i, 1] < -80]
    corners = {}
    for sx in (-1, 1):
        side = [i for i in lip_line if co[i, 0] * sx > 5]
        corners[sx] = max(side, key=lambda i: co[i, 0] * sx)
        report(f"corner {sx:+d}: vertex {corners[sx]} at {np.round(co[corners[sx]], 1)}")
    # lips: outer-face vertices within 4 edge steps of the lip_line, in front
    lipline_ok = np.array([co[i, 1] < -85 and not in_set(i, {11}) for i in range(n)])
    lipd = rings_from(bm, lip_line, 4, lipline_ok)
    lip = [v for v in lipd if not in_set(v, {7}) or v in lip_line]
    report(f"lip_line {len(lip_line)} vertices, lips {len(lip)} vertices")

    # 2. lower-lid margins
    margins = {}
    for sx, lid, sock in ((-1, 9, 5), (1, 10, 6)):
        m = [i for i in range(n) if in_set(i, {lid}) and in_set(i, {sock}) and co[i, 2] < -1.5 and front[i]]
        margins[sx] = m
        report(f"lower-lid margin {sx:+d}: {len(m)} vertices, z {co[m, 2].min():.1f}..{co[m, 2].max():.1f}")

    move = np.zeros((n, 3))
    touched = np.zeros(n)
    outer = np.array([front[i] and not in_set(i, {5, 6, 11}) and co[i, 1] < -50 for i in range(n)])

    for sx in (-1, 1):
        k = 1.0 if sx == LEFT else 0.55
        # corner: rings 0..4 on the outer surface and lips
        d = rings_from(bm, [corners[sx]], 4, outer)
        for v, w in falloff(d, 4).items():
            move[v] += k * w * np.array([sx * 1.2, 1.4, 2.6])
            touched[v] = max(touched[v], w)
        # cheek: seeds are the outer-surface vertices 3 rings from the corner that are
        # above it, grown up to 7 rings without entering lips, lids or nose
        cheek_ok = np.array([outer[i] and not in_set(i, {7, 9, 10}) for i in range(n)])
        # the seeds are the cheek-side neighbours of the lip region's outer ring, beside
        # and above the corner: where the nasolabial fold starts
        ring3 = sorted({e.other_vert(bm.verts[v]).index for v, dd in lipd.items() if dd >= 3
                        for e in bm.verts[v].link_edges
                        if cheek_ok[e.other_vert(bm.verts[v]).index]
                        and e.other_vert(bm.verts[v]).index not in lipd}
                       - set(), key=int)
        ring3 = [v for v in ring3 if co[v, 0] * sx > abs(co[corners[sx], 0]) - 2 and co[v, 2] > -80]
        dc = rings_from(bm, ring3, 7, cheek_ok)
        for v, w in falloff(dc, 7).items():
            nv = nrm[v]
            move[v] += k * w * (np.array([0.0, 0.0, 2.6]) + nv * 2.2) * CHEEK
            touched[v] = max(touched[v], w)
        report(f"cheek {sx:+d}: {len(ring3)} seeds, {len(dc)} vertices")
        # lower lid: margin and 3 rings below it, through lid and cheek vertices
        lid_ok = np.array([outer[i] or in_set(i, {9, 10}) for i in range(n)]) & ~np.array([in_set(i, {5, 6}) and not in_set(i, {9, 10}) for i in range(n)])
        dl = rings_from(bm, margins[sx], 4, lid_ok)
        for v, w in falloff(dl, 4).items():
            if co[v, 2] > 0.5:
                continue
            move[v] += k * w * np.array([0.0, -0.3, 1.5])
            touched[v] = max(touched[v], w)
        report(f"lower lid {sx:+d}: {len(dl)} vertices")

    # lips: relax the pucker, strongest in the middle
    for i in lip:
        c = max(0.0, 1.0 - abs(co[i, 0]) / 30.0) * (1.0 - lipd[i] / 5.0)
        move[i] += -nrm[i] * 0.7 * c
        touched[i] = max(touched[i], c)

    new = co + move * strength
    for v in bm.verts:
        v.co = new[v.index] / 1000.0
    bm.to_mesh(me)
    bm.free()
    me.update()

    # 4. masked smooth over the edited rings only
    win = bpy.context.window_manager.windows[0]
    area = next(a for a in win.screen.areas if a.type == "VIEW_3D")
    reg = next(r for r in area.regions if r.type == "WINDOW")
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    with bpy.context.temp_override(window=win, area=area, region=reg, active_object=obj, object=obj,
                                   selected_objects=[obj]):
        bpy.ops.object.mode_set(mode="SCULPT")
        a = me.attributes.get(".sculpt_mask") or me.attributes.new(".sculpt_mask", "FLOAT", "POINT")
        a.data.foreach_set("value", (1.0 - 0.6 * np.clip(touched, 0, 1)).astype(np.float32))
        bpy.ops.sculpt.mesh_filter("EXEC_DEFAULT", type="SMOOTH", strength=0.5, iteration_count=1)
        bpy.ops.object.mode_set(mode="OBJECT")
    if ".sculpt_mask" in me.attributes:
        me.attributes.remove(me.attributes[".sculpt_mask"])
    report(f"moved {int((np.linalg.norm(move, axis=1) > 0.05).sum())} vertices")
