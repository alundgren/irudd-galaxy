"""Split-flap board built from concept slot textures. Used by relight.py (job 'board').

Each slot becomes:  static top half (current card's top), static bottom half (current card's bottom),
and per flip one flap: front = old card's top, back = new card's bottom, hinged on the middle line,
rotating 180° down. Visibility is keyframed so each half shows exactly when a real board would.
"""
import math
import bpy
from mathutils import Vector


def _plane(name, x0, x1, z0, z1, y, uvs, mat, facing_viewer=True):
    """Quad in the XZ plane at depth y. uvs: (u0,u1,v_at_z0,v_at_z1). Normal -Y (viewer) or +Y."""
    me = bpy.data.meshes.new(name)
    verts = [(x0, y, z0), (x1, y, z0), (x1, y, z1), (x0, y, z1)]
    face = [0, 1, 2, 3] if facing_viewer else [0, 3, 2, 1]
    me.from_pydata(verts, [], [face])
    uv = me.uv_layers.new(name="UVMap")
    u0, u1, va, vb = uvs
    per_vert = {0: (u0, va), 1: (u1, va), 2: (u1, vb), 3: (u0, vb)}
    for li, vi in zip(me.polygons[0].loop_indices, face):
        uv.data[li].uv = per_vert[vi]
    me.materials.append(mat)
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    return ob


def _show(ob, frames_visible, f_end):
    """Keyframe hide_render/hide_viewport so `ob` is visible only on [a, b) intervals."""
    vis = lambda f: any(a <= f < b for a, b in frames_visible)
    prev = None
    for f in range(1, f_end + 2):
        v = vis(f)
        if v != prev:
            ob.hide_render = not v
            ob.hide_viewport = not v
            ob.keyframe_insert("hide_render", frame=f)
            ob.keyframe_insert("hide_viewport", frame=f)
            prev = v


def build(slot_ob, cards, mat_for_card, flip_starts, flip_len, f_end):
    """cards: list of card keys (len = flips + 1); mat_for_card(key) -> material.
    flip_starts: frame each flip k (1..n) begins. Returns created objects."""
    bpy.context.view_layer.update()
    bb = [slot_ob.matrix_world @ Vector(c) for c in slot_ob.bound_box]
    x0, x1 = min(v.x for v in bb), max(v.x for v in bb)
    z0, z1 = min(v.z for v in bb), max(v.z for v in bb)
    y = min(v.y for v in bb) - 0.003
    zm = (z0 + z1) / 2
    h = z1 - z0
    made = []
    n = len(cards) - 1
    starts = [1] + list(flip_starts)            # card k becomes "current top" at starts[k]
    ends_top = list(flip_starts) + [f_end + 1]
    for k, key in enumerate(cards):
        m = mat_for_card(key)
        top = _plane(f"{slot_ob.name}_top{k}", x0, x1, zm, z1, y + 0.0005, (0, 1, 0.5, 1.0), m)
        _show(top, [(starts[k], ends_top[k])], f_end)
        b_start = 1 if k == 0 else flip_starts[k - 1] + flip_len
        b_end = f_end + 1 if k == n else flip_starts[k] + flip_len
        bot = _plane(f"{slot_ob.name}_bot{k}", x0, x1, z0, zm, y + 0.0005, (0, 1, 0.0, 0.5), m)
        _show(bot, [(b_start, b_end)], f_end)
        made += [top, bot]
    for k in range(1, n + 1):
        fs = flip_starts[k - 1]
        hinge = bpy.data.objects.new(f"{slot_ob.name}_hinge{k}", None)
        bpy.context.scene.collection.objects.link(hinge)
        hinge.location = (0, y - 0.001, zm)
        # front: old card's top half; back: new card's bottom half, laid out so it reads upright after 180°
        front = _plane(f"{slot_ob.name}_flapF{k}", x0, x1, 0, z1 - zm, 0, (0, 1, 0.5, 1.0), mat_for_card(cards[k - 1]))
        back = _plane(f"{slot_ob.name}_flapB{k}", x0, x1, 0, z1 - zm, 0.0002, (0, 1, 0.5, 0.0), mat_for_card(cards[k]),
                      facing_viewer=False)
        for p in (front, back):
            p.parent = hinge
            _show(p, [(fs, fs + flip_len)], f_end)
        for f in range(fs, fs + flip_len + 1):
            t = (f - fs) / flip_len
            ease = t * t * (3 - 2 * t)            # flaps start slow and slap down
            hinge.rotation_euler = (math.pi * ease, 0, 0)  # +X: top edge swings toward the viewer (-Y)
            hinge.keyframe_insert("rotation_euler", frame=f)
        made += [front, back, hinge]
    slot_ob.hide_render = True
    slot_ob.hide_viewport = True
    return made
