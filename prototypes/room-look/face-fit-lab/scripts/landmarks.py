# Landmarks: the concept's points (2D) and the matching points on the head (3D).
#
# CONCEPT is measured by eye on zoomed, gridded crops of the comparison crop CROP (see
# below). Each point has:
#   conf  - estimated 1-sigma uncertainty in crop pixels (how sure the mark is);
#   role  - "anchor": skull points (eye corners, nose bridge, ear), used to fit the camera;
#           "form":   fixed anatomy whose proportions differ between the base head and the
#                     concept (nose, chin); not used for the camera, used by arm C;
#           "expr":   moved by the concept's expression (smile, squint, gaze); excluded from
#                     the camera fit, used by the bounded geometry fit (arm C);
#           "paint":  carried by paint, not by geometry (brows); reported only.
# "R" is his right (the near side, -X in Blender), "L" his left (far side, +X).
#
# Model points are vertex ids on Bram_Head (topology from the CC0 base head's face sets)
# so the same ids can be measured again on any shape key. Pupils are points on the eye
# meshes. Brows come from the removed brow-hair mesh of the Bram lab source (measured
# once from it; values below), snapped to the nearest head vertex.
import json

import numpy as np

# Comparison crop: rows 2..902, columns 1100..1668 of the reference image (568 x 900),
# i.e. the right-hand face study panel including its frame.
CROP = (2, 902, 1100, 1668)
REFERENCE = "game/docs/visual-upgrade/references/room-and-face.png"

CONCEPT = {
    # eyes: corners are the canthi where upper and lower lid margins meet
    "eye_R_outer": ((263.8, 298.8), 3, "anchor"),
    "eye_R_inner": ((319.5, 319.5), 3, "anchor"),
    "eye_L_inner": ((381.0, 335.0), 4, "anchor"),
    "eye_L_outer": ((420.0, 356.0), 5, "anchor"),
    "pupil_R": ((285.0, 301.0), 3, "expr"),
    "pupil_L": ((391.0, 342.5), 4, "expr"),
    "lid_R_upper": ((287.5, 291.0), 3, "expr"),
    "lid_R_lower": ((290.0, 311.0), 3, "expr"),
    "lid_L_upper": ((397.5, 332.5), 4, "expr"),
    "lid_L_lower": ((402.5, 357.0), 5, "expr"),
    # brows: painted hair bands
    "brow_R_inner": ((355.0, 283.0), 6, "paint"),
    "brow_R_peak": ((297.0, 230.0), 6, "paint"),
    "brow_R_outer": ((243.0, 255.0), 8, "paint"),
    "brow_L_inner": ((387.0, 300.0), 6, "paint"),
    "brow_L_outer": ((440.0, 311.0), 8, "paint"),
    # nose
    "nasion": ((362.0, 312.0), 10, "anchor"),
    "nose_tip": ((392.0, 393.0), 8, "form"),
    "alar_R": ((302.0, 386.0), 4, "form"),
    "subnasale": ((374.0, 420.0), 8, "form"),
    # mouth (smiling in the concept)
    "mouth_R": ((260.0, 432.0), 4, "expr"),
    "mouth_L": ((377.0, 465.0), 6, "expr"),
    "lip_upper": ((365.0, 438.0), 8, "expr"),
    "stomion": ((368.0, 450.0), 8, "expr"),
    "lip_lower": ((365.0, 466.0), 8, "expr"),
    # chin: hidden under the beard, so loose
    "chin": ((318.0, 565.0), 15, "form"),
    # near ear
    "ear_top": ((134.0, 275.0), 5, "anchor"),
    "ear_back": ((101.0, 307.0), 6, "expr"),   # silhouette point, not a fixed surface point
    "ear_lobe": ((137.5, 384.0), 5, "anchor"),
    "tragus": ((148.0, 333.0), 8, "anchor"),
}

# Measured once from the brow-hair mesh of the Bram lab source (world mm), before it was
# removed from this lab's copy: inner end, highest point, outer end per side.
BROW_MM = {
    "brow_R_inner": (-8.9, -95.7, 18.3), "brow_R_peak": (-28.5, -92.1, 27.7),
    "brow_R_outer": (-61.4, -63.3, 8.7),
    "brow_L_inner": (8.8, -95.7, 18.3), "brow_L_outer": (61.4, -62.7, 6.9),
}


def eye_separation(pts=None):
    """Concept distance between the two eye centres (mid-point of each eye's corners)."""
    c = {k: np.array(v[0]) for k, v in CONCEPT.items()} if pts is None else pts
    r = (c["eye_R_outer"] + c["eye_R_inner"]) / 2
    l = (c["eye_L_outer"] + c["eye_L_inner"]) / 2
    return float(np.linalg.norm(r - l))


def pick_model_ids(bpy):
    """Choose head vertex ids for each landmark (rules below). Returns {name: id or ('eye', name)}."""
    import bmesh
    obj = bpy.data.objects["Bram_Head"]
    me = obj.data
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.verts.ensure_lookup_table()
    n = len(bm.verts)
    co = np.array([v.co[:] for v in bm.verts]) * 1000.0
    fs = np.empty(len(me.polygons), dtype=np.int32)
    me.attributes[".sculpt_face_set"].data.foreach_get("value", fs)
    vfs = [set() for _ in range(n)]
    for f in bm.faces:
        for v in f.verts:
            vfs[v.index].add(int(fs[f.index]))
    has = lambda i, s: bool(vfs[i] & s)
    ids = {}
    # eye corners and lid mid-points are measured on the eyeball where the lids leave it
    # uncovered (see eye_aperture); they are recomputed on whatever geometry is measured.
    for side in ("R", "L"):
        for part in ("inner", "outer"):
            ids[f"eye_{side}_{part}"] = ["aperture", side, part]
        for part in ("upper", "lower"):
            ids[f"lid_{side}_{part}"] = ["aperture", side, part]
    # mouth corners: outermost vertex where the inner mouth set (7) meets the outer face
    lip_line = [i for i in range(n) if has(i, {7}) and has(i, {24, 33}) and co[i, 1] < -80]
    for side, sx in (("R", -1), ("L", 1)):
        ids[f"mouth_{side}"] = max((i for i in lip_line if co[i, 0] * sx > 5), key=lambda i: co[i, 0] * sx)
    # midline points: vertices within 1.5 mm of x = 0, front of face
    mid = np.array([i for i in range(n) if abs(co[i, 0]) < 1.5 and co[i, 1] < -40])

    def front_in(z0, z1, pick="front", extra=None):
        c = [i for i in mid if z0 <= co[i, 2] <= z1 and (extra is None or extra(i))]
        return (min if pick == "front" else max)(c, key=lambda i: co[i, 1])
    ids["nose_tip"] = front_in(-60, -15)
    tz = co[ids["nose_tip"], 2]
    ids["subnasale"] = front_in(tz - 22, tz - 4, "back", lambda i: not has(i, {7}))
    sz = co[ids["subnasale"], 2]
    # lips: most forward midline point of each lip's outer surface
    ids["lip_upper"] = front_in(sz - 20, sz - 3, extra=lambda i: not has(i, {7}))
    # stomion: mid-point of the upper and lower inner lip edges at the midline
    up_edge = min((i for i in lip_line if abs(co[i, 0]) < 1.0), key=lambda i: -co[i, 2])
    lo_edge = min((i for i in lip_line if abs(co[i, 0]) < 1.0), key=lambda i: co[i, 2])
    ids["stomion"] = [up_edge, lo_edge]
    stz = (co[up_edge, 2] + co[lo_edge, 2]) / 2
    ids["lip_lower"] = front_in(stz - 16, stz - 2, extra=lambda i: not has(i, {7}))
    ids["chin"] = front_in(-128, co[ids["lip_lower"], 2] - 20)
    ids["nasion"] = front_in(-6, 14, "back", lambda i: co[i, 1] < -80)
    # nose wing: widest point of the nose set on his right
    nose = [i for i in range(n) if has(i, {11})]
    ids["alar_R"] = min(nose, key=lambda i: co[i, 0])
    # near ear (set 3, his right)
    ear = [i for i in range(n) if has(i, {3}) and not has(i, {33})]
    ids["ear_top"] = max(ear, key=lambda i: co[i, 2])
    ids["ear_lobe"] = min(ear, key=lambda i: co[i, 2])
    ids["ear_back"] = max(ear, key=lambda i: co[i, 1] - 0.3 * abs(co[i, 2] - co[ids["ear_top"], 2] + 25))
    # tragus: the most forward ear vertex in the ear's middle third
    zt, zl = co[ids["ear_top"], 2], co[ids["ear_lobe"], 2]
    midear = [i for i in ear if zl + 0.35 * (zt - zl) < co[i, 2] < zl + 0.6 * (zt - zl)]
    ids["tragus"] = min(midear, key=lambda i: co[i, 1])
    # brows: nearest head vertex to the measured brow-hair points
    for k, p in BROW_MM.items():
        ids[k] = int(np.argmin(((co - np.array(p)) ** 2).sum(1)))
    ids["pupil_R"] = ("eye", "Bram_Eye_R")
    ids["pupil_L"] = ("eye", "Bram_Eye_L")
    bm.free()
    return {k: ([int(x) if not isinstance(x, str) else x for x in v] if isinstance(v, (list, tuple)) else int(v))
            for k, v in ids.items()}


def pupil_world(bpy, eye_name):
    """World position (m) of the pupil: the eye vertex at the dark pupil of its texture
    (the UV sphere's pole that the painted pupil sits on)."""
    o = bpy.data.objects[eye_name]
    me = o.data
    uv = np.empty(len(me.loops) * 2, dtype=np.float32)
    me.uv_layers[0].data.foreach_get("uv", uv)
    uv = uv.reshape(-1, 2)
    vi = np.empty(len(me.loops), dtype=np.int32)
    me.loops.foreach_get("vertex_index", vi)
    best = vi[int(np.argmax(uv[:, 1]))]
    return np.array(o.matrix_world @ me.vertices[int(best)].co)


def model_points(bpy, ids, coords=None):
    """World positions (m) of the landmarks. coords: optional (n,3) local vertex positions to use
    instead of the mesh's own (e.g. a shape key)."""
    obj = bpy.data.objects["Bram_Head"]
    me = obj.data
    if coords is None:
        co = np.empty(len(me.vertices) * 3, dtype=np.float32)
        me.vertices.foreach_get("co", co)
        coords = co.reshape(-1, 3)
    m = np.array(obj.matrix_world)
    out = {}
    ap = {}
    for k, v in ids.items():
        if isinstance(v, list) and v[0] == "eye":
            out[k] = pupil_world(bpy, v[1])
        elif isinstance(v, list) and v[0] == "aperture":
            if v[1] not in ap:
                ap[v[1]] = eye_aperture(bpy, coords, v[1])
            out[k] = ap[v[1]][v[2]]
        elif isinstance(v, list):
            out[k] = m[:3, :3] @ coords[v].mean(0) + m[:3, 3]
        else:
            out[k] = m[:3, :3] @ coords[v] + m[:3, 3]
    return out


def eye_aperture(bpy, coords, side):
    """Eye opening measured on the eyeball: sample its front half and keep the points
    whose outward ray leaves without hitting the head within 25 mm (not under a lid).
    Corners are the uncovered points furthest along x towards / away from the nose; lid
    points are the highest / lowest uncovered points within 2 mm of the opening's middle.
    Returns world positions (m)."""
    from mathutils import Vector
    from mathutils.bvhtree import BVHTree
    obj = bpy.data.objects["Bram_Head"]
    eye = bpy.data.objects["Bram_Eye_R" if side == "R" else "Bram_Eye_L"]
    m = np.array(obj.matrix_world)
    world = coords @ m[:3, :3].T + m[:3, 3]
    polys = [tuple(p.vertices) for p in obj.data.polygons]
    tree = BVHTree.FromPolygons([tuple(p) for p in world], polys)
    c = np.array(eye.matrix_world.translation)
    r = eye.dimensions[0] / 2
    sx = -1 if side == "R" else 1
    pts = []
    for el in np.radians(np.arange(-60, 61, 1.5)):
        for az in np.radians(np.arange(-80, 81, 1.5)):
            d = np.array([np.sin(az) * np.cos(el), -np.cos(az) * np.cos(el), np.sin(el)])
            p = c + d * (r + 0.0002)
            hit = tree.ray_cast(Vector(p), Vector(d), 0.025)
            if hit[0] is None:
                pts.append(p)
    pts = np.array(pts)
    medial = pts[np.argmin(pts[:, 0] * sx)]
    lateral = pts[np.argmax(pts[:, 0] * sx)]
    cx = (medial[0] + lateral[0]) / 2
    band = pts[np.abs(pts[:, 0] - cx) < 0.002]
    return {"inner": medial, "outer": lateral, "upper": band[np.argmax(band[:, 2])],
            "lower": band[np.argmin(band[:, 2])], "points": pts}


def save(path, obj):
    with open(path, "w") as f:
        json.dump(obj, f, indent=1)
