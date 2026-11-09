# Checkpoint 4: Bram's head from the CC0 "Head (Animation) - Realistic" base mesh
# (Human Base Meshes v1.4.1, see SOURCES.md), adapted towards Bram with direct edits.
#
# prepare(): copy the untouched HBM_Head_Original into Bram_Head, apply its one Multires
#   level (the same quad topology, subdivided once), scale it by 1.04 about the point
#   between the eyes so it matches Bram's head size, and put a Subdivision modifier on it.
# identity(): sculpt-style edits on named regions (grab / push / flatten / crease /
#   masked Sculpt Mode smoothing, see sculpt_cp3.Sculpt), written straight into the mesh.
# Rerunning prepare() then identity() always starts again from the untouched original.
import bpy
import numpy as np
from mathutils import Matrix, Vector

import sculpt_cp3

SCALE = 1.04
PIVOT = Vector((0.0, -0.0725, 0.0))


def prepare():
    src = bpy.data.objects["HBM_Head_Original"]
    old = bpy.data.objects.get("Bram_Head")
    mats = list(old.data.materials) if old else []
    if old:
        om = old.data
        bpy.data.objects.remove(old)
        bpy.data.meshes.remove(om)
    me = src.data.copy()
    me.name = "Bram_Head_mesh"
    obj = bpy.data.objects.new("Bram_Head", me)
    bpy.data.collections["Bram_Asset"].objects.link(obj)
    obj.matrix_world = src.matrix_world.copy()
    mr = obj.modifiers.new("Multires", "MULTIRES")
    mr.levels = mr.sculpt_levels = mr.render_levels = 1
    win = bpy.context.window_manager.windows[0]
    area = next(a for a in win.screen.areas if a.type == "VIEW_3D")
    reg = next(r for r in area.regions if r.type == "WINDOW")
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    with bpy.context.temp_override(window=win, area=area, region=reg, active_object=obj, object=obj,
                                   selected_objects=[obj]):
        bpy.ops.object.modifier_apply(modifier=mr.name)
    S = Matrix.Translation(PIVOT) @ Matrix.Scale(SCALE, 4) @ Matrix.Translation(-PIVOT)
    obj.data.transform(S @ obj.matrix_world)
    obj.matrix_world = Matrix.Identity(4)
    obj.data.shade_smooth()
    if ".sculpt_mask" in obj.data.attributes:
        obj.data.attributes.remove(obj.data.attributes[".sculpt_mask"])
    uv = obj.data.uv_layers.get("UVMap")
    if uv:
        uv.name = "UVMap_HBM"
    # The base mesh continues into the shoulders; below the collar it is hidden by a
    # Mask modifier (vertex group), so the mesh itself stays whole.
    co = np.empty(len(obj.data.vertices) * 3, dtype=np.float32)
    obj.data.vertices.foreach_get("co", co)
    c3 = co.reshape(-1, 3)
    # keep the throat and upper chest (inside the shirt); drop shoulders and back below
    keep = np.flatnonzero((c3[:, 2] > -0.170) | ((c3[:, 1] < 0.03) & (np.abs(c3[:, 0]) < 0.085)))
    vg = obj.vertex_groups.new(name="Above_Collar")
    vg.add(keep.tolist(), 1.0, "REPLACE")
    mask = obj.modifiers.new("Hide below collar", "MASK")
    mask.vertex_group = vg.name
    sub = obj.modifiers.new("Subdivision", "SUBSURF")
    sub.levels = 1
    sub.render_levels = 2
    obj.data.materials.clear()
    for m in mats[:1] or [bpy.data.materials.get("M_Bram_Skin")]:
        if m:
            obj.data.materials.append(m)
    for name, sx in (("Bram_Eye_L", 1), ("Bram_Eye_R", -1)):
        e = bpy.data.objects[name]
        e.location = (sx * 0.0323 * SCALE, -0.0725, 0.0)
        e.scale = (0.012 * SCALE / 0.0124,) * 3
    return obj


def identity():
    s = sculpt_cp3.Sculpt("Bram_Head", None)
    s.snap = True   # cp5 and later: region centres sit on the skin (cp4 itself ran without)
    s.co = s.base.copy()
    L = 1  # his left (+X), the side of the half smile

    # Nose: shorter, broad and rounded, with full wings; a lower, wider bridge
    s.grab("nose base up", (0, -104, -50), (26, 22, 20), (0, 0.8, 3.6))
    for sx in (-1, 1):
        s.grab("wings wider", (sx * 16, -99, -48), (10, 11, 10), (sx * 5.0, 0.6, 0.4))
        s.push("wings fuller", (sx * 18, -99, -48), (8, 9, 8), 2.0)
        s.grab("tip broader", (sx * 5, -114, -40), (8, 9, 9), (sx * 2.6, 0, 0))
        s.grab("bridge wider", (sx * 6, -100, -18), (7, 9, 15), (sx * 1.6, 0, 0))
    s.push("tip rounder", (0, -117, -39), (11, 10, 10), 1.6)
    s.push("bridge lower", (0, -101, -16), (7, 8, 14), -1.5)

    # Mouth: up a little (the nose base went up), fuller lips, wider
    s.grab("mouth up", (0, -100, -76), (34, 26, 20), (0, 0.6, 2.4))
    s.push("upper lip fuller", (0, -103, -66), (23, 9, 6), 2.4)
    s.push("lower lip fuller", (0, -102, -80), (22, 10, 7), 3.0)
    for sx in (-1, 1):
        s.grab("mouth wider", (sx * 23, -96, -73), (8, 9, 8), (sx * 3.0, 0, 0))

    # Brow: heavy ridge, brows sitting lower over deeper-set eyes
    for sx in (-1, 1):
        s.push("brow ridge", (sx * 25, -90, 20), (24, 11, 9), 3.0)
        s.grab("brow lower", (sx * 30, -88, 17), (19, 11, 8), (0, -0.5, -2.0))
    s.push("glabella", (0, -94, 14), (11, 9, 9), 2.0)

    # Cheekbones broad and forward; flatter planes below them; jaw and chin broad and
    # square; a longer chin; thicker neck
    for sx in (-1, 1):
        s.grab("cheekbone out", (sx * 52, -70, -18), (18, 20, 15), (sx * 4.0, -2.0, 0.5))
        s.push("plane below cheekbone", (sx * 50, -72, -50), (14, 16, 14), -1.8)
        s.grab("jaw angle out", (sx * 55, -10, -84), (22, 28, 24), (sx * 6.0, 0, -1.5), front_only=False)
        s.grab("chin wider", (sx * 14, -93, -106), (11, 11, 13), (sx * 3.5, 0, 0))
        s.grab("neck thicker", (sx * 50, 0, -150), (32, 70, 55), (sx * 7.0, 0, 0), front_only=False)
    s.grab("chin forward and down", (0, -95, -106), (24, 16, 18), (0, -3.0, -3.5))

    # Age: nasolabial folds, soft bags under the eyes, a line where lid meets cheek
    for sx in (-1, 1):
        deep = 1.6 if sx == L else 1.1
        s.crease("nasolabial fold", [(sx * 22, -101, -48), (sx * 29, -97, -60), (sx * 33, -92, -73)], 3.0, deep,
                 lift_side=(sx, -0.4, 0), lift=0.9)
        s.push("under-eye bag", (sx * 34, -83, -15), (13, 6, 4), 0.9)
        s.crease("lid-cheek junction", [(sx * 25, -86, -19), (sx * 34, -84, -21), (sx * 43, -79, -18)], 1.8, 0.5)

    # Half smile on his left: corner up, out and back; cheek bunches; lower lid lifts
    s.grab("smile corner", (27, -96, -73), (10, 10, 9), (1.4, 2.0, 4.5))
    s.grab("smile cheek", (36, -86, -44), (16, 13, 13), (0.5, -1.6, 2.6))
    s.grab("smile lower lid", (33.6, -82, -12), (9, 6, 4), (0, -0.3, 1.1))
    s.grab("other corner level", (-27, -96, -73), (8, 9, 8), (0, 0.4, 0.8))

    # Second pass: deep-set eyes with a clear upper-lid crease; a less projecting, rounder
    # tip; a wider nose base where the wings meet the cheek; a shorter upper lip.
    for sx in (-1, 1):
        arc = []
        for t in np.linspace(-1, 1, 9):
            dx = 11.0 * t
            z = 7.6 + 1.6 * (1 - t * t) - 0.6 * t * sx * 0  # lid crease about 8 mm above the eye centre
            r = 14.2
            dy = np.sqrt(max(r * r - dx * dx - z * z, 1.0))
            arc.append((sx * 33.6 + sx * dx, -72.5 - dy, z))
        s.crease("upper-lid crease", arc, 1.4, 1.1, lift_side=(0, 0, 1), lift=0.5)
        s.push("deep-set socket", (sx * 33.6, -86, 9), (14, 6, 5), -0.8)
        s.grab("nose base wider", (sx * 20, -97, -51), (7, 8, 7), (sx * 2.0, 0, 0))
        s.grab("bridge root wider", (sx * 5, -96, -4), (6, 8, 10), (sx * 1.4, 0, 0))
    s.grab("tip less projecting", (0, -118, -38), (12, 11, 11), (0, 2.6, -0.4))
    s.push("tip rounder again", (0, -116, -39), (9, 8, 8), 0.8)
    s.grab("mouth up again", (0, -100, -76), (34, 26, 20), (0, 0, 1.5))

    s.smooth("blend the edits", (0, -85, -45), (70, 40, 75), iterations=2, strength=0.3, eye_margin=1.5)
    s.commit()
    return s.log


def cp5_expression():
    """Checkpoint 5, after prepare() and identity(): the expression and nose corrections
    asked for at the cp4 review."""
    s = sculpt_cp3.Sculpt("Bram_Head", None)
    s.snap = True
    s.co = s.base.copy()

    # Brows: relax the knit between them, raise his right brow (-X) into a clear arch,
    # leave his left brow level and relaxed.
    for sx in (-1, 1):
        s.grab("inner brow eased", (sx * 11, -93, 14), (11, 9, 9), (0, -0.2, 0.6))
    s.smooth("relax knit between brows", (0, -92, 24), (30, 10, 13), iterations=4, strength=0.7, eye_margin=3.0)
    s.grab("raised right brow", (-35, -88, 19), (15, 11, 10), (0, -0.3, 3.0))
    s.grab("right brow tail", (-42, -82, 16), (9, 9, 8), (0, 0, 1.5))
    s.grab("left brow relaxed", (32, -88, 18), (15, 10, 8), (0, 0, -0.5))

    # Mouth: corners no longer turned down; his left corner lifted into the cheek,
    # his right corner relaxed level; the corners blend into the cheeks.
    s.grab("left corner up", (27, -96, -73), (9, 9, 8), (0.5, 1.0, 2.5))
    s.grab("right corner level", (-27, -96, -74), (8, 9, 8), (-0.8, 0.8, 3.0))
    s.grab("right cheek slight lift", (-34, -88, -44), (13, 12, 11), (0, -0.4, 0.8))
    for sx in (-1, 1):
        s.smooth("corner into cheek", (sx * 28, -95, -73), (8, 8, 8), iterations=3, strength=0.6)
    s.grab("left cheek lifted", (34, -88, -42), (15, 13, 12), (0.3, -1.0, 1.6))
    s.grab("left lower lid support", (33.6, -82, -11), (9, 6, 4), (0, -0.2, 0.7))
    # Lower lip: less of a separate bulge, its lower edge blends into the chin
    s.push("lower lip bulge back", (0, -103, -83), (19, 8, 5), -1.2)
    s.smooth("lip to chin", (0, -99, -89), (20, 7, 6), iterations=4, strength=0.7)
    s.push("lower lip a little less", (0, -102, -80), (21, 9, 6), -1.1)
    s.smooth("chin into lip", (0, -96, -100), (22, 10, 11), iterations=3, strength=0.6)

    # Nose: a broad rounded tip that projects less, fuller wings, a wider supratip
    for sx in (-1, 1):
        s.grab("tip wider", (sx * 6, -112, -40), (7, 8, 8), (sx * 1.6, 0, 0))
        s.grab("supratip wider", (sx * 5, -110, -30), (6, 8, 8), (sx * 1.3, 0, 0))
        s.push("wings fuller again", (sx * 19, -97, -47), (8, 8, 7), 0.8)
    s.push("tip lobule fuller", (0, -111, -44), (10, 8, 6), 1.0)
    s.grab("tip projects less", (0, -117, -38), (10, 9, 9), (0, 1.5, -0.3))

    # Mature folds: soft and broad, no deep grooves
    for sx in (-1, 1):
        s.crease("soft nasolabial", [(sx * 21, -100, -47), (sx * 28, -96, -59), (sx * 32, -91, -71)], 4.0, 0.4)
        s.crease("soft tear trough", [(sx * 24, -88, -14), (sx * 31, -86, -18), (sx * 38, -84, -19)], 2.6, 0.3)
    # Smooth the furrow left above the brows and the small dimple on the cheeks
    s.smooth("forehead furrow", (0, -90, 32), (45, 12, 8), iterations=3, strength=0.5, eye_margin=3.0)
    for sx in (-1, 1):
        s.smooth("cheek dimple", (sx * 38, -90, -45), (14, 12, 14), iterations=3, strength=0.5)
    # A clearer half smile: both corners drawn back into the cheeks and up (his left
    # more), the lips stretched a little flatter over the teeth, the cheeks above the
    # corners fuller. No new grooves.
    s.grab("left corner back and up", (28, -95, -71), (9, 9, 8), (1.5, 2.2, 3.0))
    s.grab("right corner back and up", (-28, -95, -72), (9, 9, 8), (-1.2, 1.8, 2.2))
    s.push("lips stretched", (0, -104, -74), (22, 8, 9), -1.2)
    for sx, amt in ((1, 0.9), (-1, 0.5)):
        s.push("cheek above corner", (sx * 31, -92, -60), (10, 9, 9), amt)
    s.smooth("mouth corners settle", (0, -98, -72), (34, 12, 12), iterations=2, strength=0.4)
    s.commit()
    return s.log


def cp6_expression():
    """Checkpoint 6, after cp5_expression(): one warm half smile carried by the corner, the
    cheek and the lower lid together (his left strongest, his right softer); a rounder,
    lower raised brow; a lower lip that no longer projects; fuller mature volume under
    the eyes and beside the folds. Soft forms only, no wrinkle lines."""
    s = sculpt_cp3.Sculpt("Bram_Head", None)
    s.snap = True
    s.co = s.base.copy()

    # Brow: the raised right brow comes down a little and its peak becomes a round arch
    s.grab("right brow rounder", (-34, -88, 21), (16, 11, 8), (0, 0, -1.1))
    s.smooth("right brow arch", (-32, -89, 19), (17, 10, 9), iterations=3, strength=0.6, eye_margin=3.0)

    # Lower lip: pulled back so it sits under the upper lip instead of projecting
    s.push("lower lip back", (0, -103, -80), (20, 9, 6), -1.3)
    s.grab("lower lip tucked", (0, -103, -81), (18, 9, 6), (0, 1.0, 0.3))
    s.smooth("lower lip into chin", (0, -100, -86), (20, 8, 7), iterations=3, strength=0.5)

    # The smile, his left (+X): cheek mass rises and comes forward over the corner, the
    # lower lid is pushed up by it, the fold beside the mouth deepens under the cheek.
    s.grab("left cheek rises", (35, -88, -38), (16, 13, 14), (0.3, -1.4, 2.6))
    s.push("left cheek fuller", (38, -88, -40), (13, 11, 11), 0.9)
    s.grab("left lower lid pushed up", (33.6, -83, -12), (11, 7, 5), (0, -0.4, 1.3))
    s.grab("left outer eye narrows", (44, -79, -6), (8, 7, 6), (0, -0.2, 0.8))
    s.crease("left fold under cheek", [(24, -100, -45), (31, -96, -57), (34, -91, -70)], 3.2, 0.5,
             lift_side=(1, -0.4, 0.2), lift=0.6)
    s.grab("left corner into cheek", (29, -95, -70), (8, 8, 7), (0.6, 0.8, 1.0))
    # His right (-X): the same, softer
    s.grab("right cheek rises", (-35, -88, -38), (15, 12, 13), (-0.2, -0.8, 1.4))
    s.grab("right lower lid up", (-33.6, -83, -12), (10, 7, 5), (0, -0.2, 0.6))
    s.grab("right corner into cheek", (-29, -95, -71), (8, 8, 7), (-0.4, 0.6, 0.8))

    # The base mesh's mouth line dips about 2 mm halfway to each corner; lifting only the
    # corners made an S-curve (a sneer). Lift the whole outer half of each side instead.
    s.grab("left half of mouth up", (22, -97, -74), (12, 10, 7), (0.4, 0.8, 3.0))
    s.grab("right half of mouth up", (-22, -97, -75), (12, 10, 7), (-0.3, 0.7, 2.6))
    # Measured after the lift: his right corner still 3.5 mm below the centre, his left
    # only level. Corner lifts aimed at those measured points.
    s.grab("right corner to level", (-26, -95, -75), (7, 8, 6), (-0.3, 0.6, 3.2))
    s.grab("left corner above level", (26, -95, -72), (7, 8, 6), (0.4, 0.8, 2.6))

    # Mature lower lids: a little more volume below the lid, a soft step to the cheek
    for sx in (-1, 1):
        s.push("lower lid volume", (sx * 32, -84, -16), (12, 6, 4), 0.5)
    # A diagonal fold at the inner brows still read as worry: smooth it out
    for sx in (-1, 1):
        s.smooth("inner brow fold", (sx * 14, -92, 12), (11, 9, 9), iterations=4, strength=0.7, eye_margin=2.5)
    s.smooth("settle the smile", (0, -92, -55), (55, 25, 40), iterations=2, strength=0.3, eye_margin=1.5)
    s.commit()
    return s.log
