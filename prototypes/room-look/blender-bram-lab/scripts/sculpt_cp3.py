# Checkpoint 3: direct edits of the existing cp2 head mesh, not a new head.
# The cp2 surface stays as the shape key "Basis"; every edit below lands in the shape key
# "cp3_sculpt", so cp2 and cp3 are both in the file and the key can be turned down or
# sculpted further by hand in Sculpt Mode.
#
# Each edit is a sculpt-style operation on a named region of the existing vertices:
#   grab     move a region with a soft falloff (like the Grab brush, proportional editing)
#   flatten  pull a region towards its own best-fit plane (like the Flatten brush)
#   push     move a region along its surface normals (Inflate / Deflate)
#   crease   cut a soft groove along a line on the surface, optionally lifting one side
#   smooth   Blender's Sculpt Mode mesh filter (Smooth / Relax) under a mask
# Regions are ellipsoids in millimetres around landmarks measured on cp2
# (face_landmarks.py). Rerunning resets cp3_sculpt to the cp2 surface first.
import bpy
import numpy as np

HEAD = "Bram_Head"
KEY = "cp3_sculpt"
EYES = [(-33.8, -72.5, 0.0), (33.8, -72.5, 0.0)]   # eyeball centres, mm
EYE_R = 12.4


class Sculpt:
    def __init__(self, head=HEAD, key=KEY):
        self.obj = bpy.data.objects[head]
        self.me = self.obj.data
        n = len(self.me.vertices)
        base = np.empty(n * 3, dtype=np.float32)
        if key is None:
            # edit the mesh itself (used for the base-mesh head, which keeps no shape keys)
            self.key = None
            self.me.vertices.foreach_get("co", base)
        else:
            if not self.me.shape_keys:
                self.obj.shape_key_add(name="Basis", from_mix=False)
            kb = self.me.shape_keys.key_blocks
            self.key = kb.get(key) or self.obj.shape_key_add(name=key, from_mix=False)
            self.key.value = 1.0
            kb["Basis"].data.foreach_get("co", base)
        self.base = base.reshape(-1, 3).astype(np.float64) * 1000.0   # mm, cp2 surface
        self.co = self.base.copy()                                     # working surface
        totals = np.empty(len(self.me.polygons), dtype=np.int32)
        self.me.polygons.foreach_get("loop_total", totals)
        loops = np.empty(len(self.me.loops), dtype=np.int32)
        self.me.loops.foreach_get("vertex_index", loops)
        starts = np.concatenate([[0], np.cumsum(totals)[:-1]])
        tris = []
        for k in range(1, totals.max() - 1):
            sel = totals > k + 1
            s = starts[sel]
            tris.append(np.stack([loops[s], loops[s + k], loops[s + k + 1]], axis=1))
        self.tris = np.concatenate(tris)
        edges = np.empty(len(self.me.edges) * 2, dtype=np.int32)
        self.me.edges.foreach_get("vertices", edges)
        self.edges = edges.reshape(-1, 2)
        self.log = []
        self.snap = False

    def on_surface(self, p):
        """With snapping on, move a point given in mm to the front surface at its X and Z
        (the most forward cp-base vertex within 2 mm), so a region is centred on the skin
        whatever depth was written."""
        if not self.snap:
            return p
        x, y, z = p
        sel = (np.abs(self.base[:, 0] - x) < 2.0) & (np.abs(self.base[:, 2] - z) < 2.0) & (self.base[:, 1] < -30)
        if not sel.any():
            return p
        return (x, float(self.base[sel, 1].min()), z)

    # -- helpers -------------------------------------------------------------
    def normals(self):
        a, b, c = (self.co[self.tris[:, i]] for i in range(3))
        fn = np.cross(b - a, c - a)
        vn = np.zeros_like(self.co)
        for i in range(3):
            np.add.at(vn, self.tris[:, i], fn)
        return vn / (np.linalg.norm(vn, axis=1, keepdims=True) + 1e-12)

    def region(self, centre, radii, front_only=True, side=None):
        """Smooth 0..1 weight inside an ellipsoid of the cp2 surface (smoothstep falloff)."""
        q = (self.base - np.array(self.on_surface(centre))) / np.array(radii)
        r = np.sqrt((q * q).sum(axis=1))
        t = np.clip(1.0 - r, 0.0, 1.0)
        w = t * t * (3 - 2 * t)
        if front_only:
            w *= np.clip((-self.base[:, 1] - 40.0) / 10.0, 0, 1)
        if side is not None:
            w *= np.clip(side * self.base[:, 0] / 2.0 + 0.5, 0, 1)
        return w

    # -- operations ----------------------------------------------------------
    def grab(self, name, centre, radii, delta, **kw):
        w = self.region(centre, radii, **kw)
        self.co += w[:, None] * np.array(delta, dtype=np.float64)
        self.log.append(f"grab {name}: {delta} mm over {radii} mm at {centre}")

    def push(self, name, centre, radii, amount, **kw):
        w = self.region(centre, radii, **kw)
        self.co += w[:, None] * self.normals() * amount
        self.log.append(f"push {name}: {amount} mm along normals over {radii} mm at {centre}")

    def flatten(self, name, centre, radii, strength, **kw):
        w = self.region(centre, radii, **kw)
        sel = w > 0.05
        p = self.co[sel]
        ww = w[sel]
        c = (p * ww[:, None]).sum(0) / ww.sum()
        u, s, vt = np.linalg.svd((p - c) * np.sqrt(ww)[:, None], full_matrices=False)
        nrm = vt[2]
        dist = (self.co - c) @ nrm
        self.co -= (w * strength * dist)[:, None] * nrm
        self.log.append(f"flatten {name}: strength {strength} over {radii} mm at {centre}")

    def crease(self, name, points, width, depth, lift_side=None, lift=0.0, region=None):
        """Soft groove along a polyline (mm, cp2 surface); optionally lift the surface on one
        side of it (a fold overhanging the groove)."""
        pts = np.array([self.on_surface(p) for p in points], dtype=np.float64)
        best = np.full(len(self.base), np.inf)
        side_off = np.zeros(len(self.base))
        for a, b in zip(pts[:-1], pts[1:]):
            ab = b - a
            t = np.clip(((self.base - a) @ ab) / (ab @ ab), 0, 1)
            proj = a + t[:, None] * ab
            d = np.linalg.norm(self.base - proj, axis=1)
            closer = d < best
            best[closer] = d[closer]
            if lift_side is not None:
                ls = np.array(lift_side, dtype=np.float64)
                side_off[closer] = (self.base[closer] - proj[closer]) @ (ls / np.linalg.norm(ls))
        g = np.exp(-(best / width) ** 2)
        if region is not None:
            g *= self.region(*region)
        nrm = self.normals()
        self.co -= (g * depth)[:, None] * nrm
        if lift_side is not None and lift:
            # a smooth swell beside the groove on the lift side; no hard edge where sides meet
            band = np.exp(-((side_off - 1.6 * width) / width) ** 2) * np.exp(-(best / (3 * width)) ** 2)
            if region is not None:
                band *= self.region(*region)
            self.co += (band * lift)[:, None] * nrm
        self.log.append(f"crease {name}: depth {depth} mm, width {width} mm, lift {lift} mm")

    def smooth(self, name, centre, radii, iterations=4, kind="SMOOTH", strength=1.0, eye_margin=2.5,
               weights=None, **kw):
        """Blender Sculpt Mode mesh filter, masked to the region (or to given weights)."""
        w = (self.region(centre, radii, **kw) if weights is None else weights) * strength
        # leave the lid margins alone: nothing within eye_margin mm of an eyeball is smoothed
        for c in EYES:
            d = np.linalg.norm(self.base - np.array(c), axis=1)
            w *= np.clip((d - (EYE_R + eye_margin)) / 1.5, 0, 1)
        self.commit()
        win = bpy.context.window_manager.windows[0]
        area = next(a for a in win.screen.areas if a.type == "VIEW_3D")
        reg = next(r for r in area.regions if r.type == "WINDOW")
        for o in bpy.context.view_layer.objects:
            o.select_set(False)
        self.obj.select_set(True)
        bpy.context.view_layer.objects.active = self.obj
        if self.key is not None:
            self.obj.active_shape_key_index = list(self.me.shape_keys.key_blocks).index(self.key)
        with bpy.context.temp_override(window=win, area=area, region=reg, active_object=self.obj,
                                       object=self.obj, selected_objects=[self.obj]):
            bpy.ops.object.mode_set(mode="SCULPT")
            m = self.me.attributes.get(".sculpt_mask") or self.me.attributes.new(".sculpt_mask", "FLOAT", "POINT")
            m.data.foreach_set("value", (1.0 - np.clip(w, 0, 1)).astype(np.float32))
            for _ in range(iterations):
                bpy.ops.sculpt.mesh_filter("EXEC_DEFAULT", type=kind, strength=1.0, iteration_count=1)
            bpy.ops.object.mode_set(mode="OBJECT")
        if ".sculpt_mask" in self.me.attributes:
            self.me.attributes.remove(self.me.attributes[".sculpt_mask"])
        out = np.empty(len(self.base) * 3, dtype=np.float32)
        (self.me.vertices if self.key is None else self.key.data).foreach_get("co", out)
        self.co = out.reshape(-1, 3).astype(np.float64) * 1000.0
        self.log.append(f"smooth {name}: {kind} x{iterations} over {radii} mm at {centre}")

    def fill_slot(self, name, path, reach):
        """Close the slot between the lips: each point within `reach` mm above or below the
        path (at the same X) is moved forward onto the straight line between the upper lip
        surface just above the slot and the lower lip surface just below it."""
        p = np.array(path, dtype=np.float64)
        order = np.argsort(p[:, 0])
        px, pz = p[order, 0], p[order, 2]
        zc = np.interp(self.base[:, 0], px, pz)
        dz = self.base[:, 2] - zc
        inside_x = (self.base[:, 0] > px[0]) & (self.base[:, 0] < px[-1]) & (self.base[:, 1] < -85)
        slot = inside_x & (np.abs(dz) < reach)
        moved = 0
        for x0 in np.arange(px[0], px[-1], 0.6):
            col = inside_x & (np.abs(self.base[:, 0] - x0) < 0.45)
            up = col & (dz > reach) & (dz < reach + 1.6)
            dn = col & (dz < -reach) & (dz > -reach - 1.6)
            sel = slot & (np.abs(self.base[:, 0] - x0) < 0.3)
            if not up.any() or not dn.any() or not sel.any():
                continue
            yu, zu = self.co[up, 1].min(), self.co[up][np.argmin(self.co[up, 1]), 2]
            yd, zd = self.co[dn, 1].min(), self.co[dn][np.argmin(self.co[dn, 1]), 2]
            idx = np.flatnonzero(sel)
            t = np.clip((self.co[idx, 2] - zd) / (zu - zd + 1e-9), 0, 1)
            target = yd + (yu - yd) * t
            behind = self.co[idx, 1] > target
            self.co[idx[behind], 1] = target[behind]
            moved += int(behind.sum())
        self.log.append(f"fill {name}: {moved} vertices brought onto the lip surface")

    def rough_band(self, box, top_percent=3.0, spread=6):
        """Where the surface is crumpled: points that sit far from the average of their
        neighbours, inside box ((x0, x1), (z0, z1), y_max), widened over the mesh."""
        co, E, n = self.base, self.edges, len(self.base)
        acc = np.zeros_like(co)
        np.add.at(acc, E[:, 0], co[E[:, 1]])
        np.add.at(acc, E[:, 1], co[E[:, 0]])
        deg = np.bincount(E.ravel(), minlength=n).astype(float)[:, None]
        rough = np.linalg.norm(co - acc / np.maximum(deg, 1), axis=1)
        (x0, x1), (z0, z1), ymax = box
        inbox = (co[:, 0] > x0) & (co[:, 0] < x1) & (co[:, 2] > z0) & (co[:, 2] < z1) & (co[:, 1] < ymax)
        hi = inbox & (rough > np.percentile(rough[co[:, 1] < ymax], 100 - top_percent))
        w = hi.astype(np.float64)
        for _ in range(spread):
            a = np.zeros(n)
            np.add.at(a, E[:, 0], w[E[:, 1]])
            np.add.at(a, E[:, 1], w[E[:, 0]])
            w = np.maximum(w, a / np.maximum(deg[:, 0], 1))
        w = np.clip(w * 2.5, 0, 1) * inbox
        centre = []
        for x in np.arange(x0 + 2, x1 - 1, 3.0):
            sel = hi & (np.abs(co[:, 0] - x) < 1.6)
            if sel.sum() > 3:
                c = co[sel]
                z = float(np.median(c[:, 2]))
                near = (np.abs(co[:, 0] - x) < 0.5) & (np.abs(co[:, 2] - z) < 0.6) & (co[:, 1] < ymax)
                centre.append((x, float(co[near, 1].min()) if near.any() else float(np.median(c[:, 1])), z))
        return w, centre

    def mouth_path(self, n=17):
        """The cp2 mouth slit as a polyline in cp2 coordinates: at each x, the deepest
        point of the surface between the lips."""
        pts = []
        for x in np.linspace(-29.0, 30.5, n):
            sel = (np.abs(self.base[:, 0] - x) < 0.6) & (self.base[:, 2] > -82) & (self.base[:, 2] < -64) \
                & (self.base[:, 1] < -92)
            band = self.base[sel]
            if len(band) == 0:
                continue
            front = band[band[:, 1] < np.percentile(band[:, 1], 60)]
            k = np.argmax(front[:, 1])
            pts.append(front[k].copy())
        p = np.array(pts)
        for _ in range(3):
            p[1:-1] = (p[:-2] + 2 * p[1:-1] + p[2:]) / 4
        return [tuple(q) for q in p]

    def keep_eyes_covered(self):
        """Lid skin must stay outside the eyeballs after edits near the eyes."""
        for c in EYES:
            v = self.co - np.array(c)
            d = np.linalg.norm(v, axis=1)
            inside = (d < EYE_R + 0.4) & (d > 1e-6)
            self.co[inside] = np.array(c) + v[inside] / d[inside, None] * (EYE_R + 0.4)

    def commit(self):
        target = self.me.vertices if self.key is None else self.key.data
        target.foreach_set("co", (self.co / 1000.0).astype(np.float32).ravel())
        self.me.update()


def run():
    s = Sculpt()
    # Start from the cp2 surface every time.
    s.co = s.base.copy()

    # 1. Shorter nose: lift the whole nose base (tip, columella, wings) and the skin under
    #    it; the bridge above takes up the change. Then pull the tip back so it is short
    #    and rounded rather than hanging forward.
    s.grab("nose base up", (0, -110, -50), (30, 26, 24), (0, 1.8, 6.0))
    s.grab("nose tip back", (0, -121, -32), (13, 10, 11), (0, 2.6, 0.8))
    # Broad, flat-fronted bridge: widen the middle of the nose and flatten its front.
    for sx in (-1, 1):
        s.grab("bridge side out", (sx * 7, -97, -16), (7, 12, 16), (sx * 1.2, -0.3, 0))
    s.flatten("bridge front", (0, -104, -18), (8, 10, 15), 0.6)
    # Wings: slightly smaller and tucked closer to the cheek
    for sx in (-1, 1):
        s.grab("nose wing in", (sx * 18, -100, -40), (8, 9, 7), (-sx * 0.8, 0.8, 0.6))

    # 2. Mouth higher and further back, lips integrated into the muzzle
    s.grab("mouth up and back", (0, -104, -78), (38, 32, 22), (0, 3.0, 4.0))
    s.push("lip roll down", (0, -110, -76), (24, 12, 11), -1.0)
    s.grab("chin forward", (0, -100, -108), (24, 16, 18), (0, -1.6, 0))

    # 3. Cheeks: take the inflated bulge down and make planes from cheekbone to fold
    for sx in (-1, 1):
        s.push("cheek deflate", (sx * 37, -86, -30), (20, 16, 18), -2.4)
        s.flatten("cheek plane", (sx * 40, -84, -32), (18, 14, 18), 0.45)
    # Upper lip barrel: the sides of the upper lip curve back round the teeth
    for sx in (-1, 1):
        s.grab("upper lip sides back", (sx * 20, -104, -60), (12, 12, 10), (0, 1.2, 0))
    s.crease("philtrum groove", [(0, -110, -52), (0, -112, -62)], 1.6, 0.45, region=((0, -110, -58), (8, 10, 12)))

    # 4. Half smile on his left (+X): corner up, out and back; cheek above it bunches up and
    #    forward; lower lid lifted by the cheek; a deeper fold beside the mouth.
    s.grab("smile corner", (30, -97, -72), (11, 11, 9), (1.0, 1.4, 3.0))
    s.grab("smile cheek bunch", (36, -88, -46), (15, 13, 13), (0.4, -1.2, 2.0))
    s.grab("smile lower lid", (34, -84, -12), (9, 8, 6), (0, -0.2, 1.1))
    s.crease("smile fold", [(24, -100, -42), (31, -97, -56), (34, -93, -68)], 3.0, 0.9,
             lift_side=(1, -0.3, 0), lift=0.6)
    s.crease("fold right", [(-24, -100, -42), (-31, -97, -56), (-33, -93, -68)], 3.0, 0.45,
             lift_side=(-1, -0.3, 0), lift=0.25)
    s.grab("right corner level", (-30, -97, -76), (9, 9, 8), (0, 0.6, 0.6))

    # 5. Brow ridge to lid to cheek: smooth the hard rings round the sockets, keeping the lid
    #    margins (masked off close to the eye opening by the region's inner limit below).
    s.keep_eyes_covered()
    for sx in (-1, 1):
        s.smooth("socket ring", (sx * 34, -80, 2), (22, 14, 18), iterations=6)
        s.smooth("cheek and fold", (sx * 34, -90, -40), (20, 14, 22), iterations=4, kind="SMOOTH", strength=0.6)
    s.smooth("nose and lip", (0, -108, -55), (22, 16, 26), iterations=3, strength=0.5)
    # 6. Second pass after looking at the clay views
    #    Sides of the face: the cheek still bulged past the cheekbone; flatten it into a
    #    plane that runs from the cheekbone down to the jaw.
    for sx in (-1, 1):
        s.push("lateral cheek in", (sx * 56, -62, -40), (14, 20, 24), -2.5)
        s.flatten("lateral cheek plane", (sx * 54, -64, -40), (14, 18, 22), 0.4)
    #    Shorter, rounder tip
    s.grab("tip back again", (0, -120, -38), (12, 9, 10), (0, 2.2, 0.4))
    s.smooth("tip round", (0, -119, -38), (9, 7, 8), iterations=2, strength=0.5)
    #    Mouth line: clear the small jagged steps left along the lips' join
    s.smooth("mouth line", (0, -106, -72), (30, 10, 6), iterations=2, strength=0.6)
    #    Soften the hard rings round the eyes, closer to the lids than before
    for sx in (-1, 1):
        s.smooth("lid rings", (sx * 34, -80, 0), (19, 12, 15), iterations=3, strength=0.5, eye_margin=1.0)
    # 7. Third pass
    #    Nostril openings sat on the lip below the wings; lift them up under the nose and
    #    tuck them back so they open downward.
    for sx in (-1, 1):
        s.grab("nostril up under nose", (sx * 9.5, -104, -51), (7, 7, 4.5), (0, 0.8, 2.6))
    #    A plane change under each cheekbone, from the side of the face towards the mouth
    #    corner: the cheek above stays full, the surface below steps back into a plane.
    for sx in (-1, 1):
        s.crease("under cheekbone", [(sx * 58, -60, -30), (sx * 50, -76, -44), (sx * 40, -88, -60)],
                 7.0, 0.6, lift_side=(0, -0.4, 1), lift=0.3)
        s.flatten("below eye plane", (sx * 36, -88, -22), (14, 10, 9), 0.35)
    #    Upper lip border: soften the rolled ridge along the top of the upper lip
    s.smooth("upper lip border", (0, -108, -63), (26, 9, 4), iterations=2, strength=0.6)
    # 8. Mouth line: close the narrow slit left by cp2 (its inner walls showed as jagged
    #    steps), then cut one clean groove along the same path on the edited surface.
    path = s.mouth_path()
    s.fill_slot("mouth slot", path, 2.4)
    s.smooth("close slit", (0, -106, path[len(path) // 2][2]), (31, 8, 3.5), iterations=12)
    s.smooth("even the closed slot", (0, -106, path[len(path) // 2][2]), (31, 7, 3.0), iterations=25, kind="RELAX")
    s.smooth("even the closed slot", (0, -106, path[len(path) // 2][2]), (31, 7, 3.0), iterations=15)
    s.crease("mouth line", path, 1.1, 1.0, region=((0, -106, -72), (34, 16, 12)))
    s.smooth("soften fold and cheekbone edges", (0, -92, -50), (62, 30, 30), iterations=2, strength=0.35)
    # 9. The mounds beside the nose wings (the old cheek-apple bulge above the fold) and
    #    the forward bulge of the cheek seen in profile: take them down and flatten them so
    #    the cheek front reads as one plane from the cheekbone to the fold.
    for sx in (-1, 1):
        keep = 0.6 if sx > 0 else 1.0   # keep more fullness on the smiling side
        s.push("mound beside nose", (sx * 40, -90, -45), (17, 14, 17), -3.4 * keep)
        s.push("side of face in", (sx * 60, -58, -50), (12, 22, 22), -2.6)
        s.flatten("cheek front plane", (sx * 42, -86, -40), (16, 12, 18), 0.35)
        s.smooth("lid rings again", (sx * 34, -80, 0), (18, 11, 14), iterations=6, strength=0.7, eye_margin=1.0)
    # 10. Nostrils: tuck the openings further back under the tip so they face down
    for sx in (-1, 1):
        s.grab("nostril back under tip", (sx * 9.6, -103, -50.5), (6, 6, 4), (0, 1.4, 1.2))
    # 11. Upper lip: the roll along its top border still stood out as a separate ridge
    s.push("upper lip ridge back", (0, -110, -64), (28, 9, 4.5), -0.8)
    s.smooth("upper lip ridge", (0, -110, -64), (28, 9, 4.5), iterations=3, strength=0.6)
    s.keep_eyes_covered()
    s.commit()
    return s.log


if __name__ == "__main__":
    for line in run():
        print(line)


def remesh_and_finish(voxel_mm=0.7):
    """After run(): rebuild the edited head as one clean surface with Blender's Voxel Remesh
    (the closed mouth slot otherwise keeps crumpled overlapping layers), then finish the
    mouth on the clean surface. The edited result replaces Bram_Head; cp2 stays in
    checkpoints/bram_bust_cp2.blend."""
    old = bpy.data.objects[HEAD]
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(old.evaluated_get(dg))
    mats = list(old.data.materials)
    old_mesh = old.data
    old.shape_key_clear()
    old.data = me
    bpy.data.meshes.remove(old_mesh)
    me.materials.clear()
    for m in mats:
        me.materials.append(m)
    me.remesh_voxel_size = voxel_mm / 1000.0
    me.use_remesh_fix_poles = True
    me.use_remesh_preserve_volume = True
    win = bpy.context.window_manager.windows[0]
    area = next(a for a in win.screen.areas if a.type == "VIEW_3D")
    reg = next(r for r in area.regions if r.type == "WINDOW")
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    old.hide_set(False)
    old.select_set(True)
    bpy.context.view_layer.objects.active = old
    with bpy.context.temp_override(window=win, area=area, region=reg, active_object=old, object=old,
                                   selected_objects=[old]):
        bpy.ops.object.voxel_remesh()
    old.data.shade_smooth()
    s = Sculpt(HEAD, "cp3_finish")
    s.log.append(f"voxel remesh at {voxel_mm} mm: {len(s.base)} vertices")
    # centre of the mouth on the edited surface: the stomion on the midline profile
    import face_landmarks
    mid_z = float(face_landmarks.measure(s.base)[0]["stomion"][0])
    band, centre = s.rough_band(((-32, 32), (-84, -60), -90))
    s.smooth("crumpled lip join", (0, -104, mid_z), (31, 9, 4.0), iterations=20, weights=band)
    # The new mouth line is drawn deliberately rather than following the old crumpled join:
    # level in the middle, his left corner (+X) lifted about 4 mm, his right corner a
    # little lower, so the half smile reads in the line itself.
    pts = np.array(s.mouth_line_on_surface(mid_z))
    # lips sit into the muzzle instead of rolling forward off it
    s.push("lower lip roll", (0, -108, mid_z - 9), (24, 10, 6), -1.0)
    s.smooth("groove under lower lip", (0, -102, mid_z - 17), (24, 8, 5), iterations=4, strength=0.6)
    s.push("upper lip roll", (0, -110, mid_z + 6), (26, 8, 4), -0.7)
    s.crease("mouth line", [tuple(p) for p in pts], 0.95, 1.5, region=((0, -104, mid_z), (36, 16, 14)))
    # corners: a small deeper pocket at each end, larger on the smiling side
    for p, depth in ((pts[0], 0.6), (pts[-1], 1.0)):
        s.crease("mouth corner", [tuple(p - np.array([0, 0, 1.5])), tuple(p + np.array([0, 0, 1.5]))], 1.4, depth,
                 region=(tuple(p), (6, 8, 6)))
    s.keep_eyes_covered()
    s.commit()
    # bake the finish into the mesh: one clean head, no keys
    dg = bpy.context.evaluated_depsgraph_get()
    obj = s.obj
    me2 = bpy.data.meshes.new_from_object(obj.evaluated_get(dg))
    om = obj.data
    obj.shape_key_clear()
    obj.data = me2
    me2.materials.clear()
    for m in mats[:1]:
        me2.materials.append(m)
    me2.name = HEAD + "_mesh"
    bpy.data.meshes.remove(om)
    me2.shade_smooth()
    return s.log


def _mouth_line_on_surface(self, mid_z, n=17):
    """A smiling mouth line on the current surface: lifted at his left corner (+X)."""
    pts = []
    for x in np.linspace(-28.5, 30.0, n):
        xr = x / 29.0
        z = mid_z + 1.0 * xr ** 4 + 3.0 * max(xr, 0.0) ** 2.2 - 3.5 * max(-xr, 0.0) ** 2.4
        sel = (np.abs(self.base[:, 0] - x) < 0.5) & (np.abs(self.base[:, 2] - z) < 0.6) & (self.base[:, 1] < -85)
        if not sel.any():
            continue
        y = self.base[sel, 1].min()
        pts.append((x, y, z))
    return pts


Sculpt.mouth_line_on_surface = _mouth_line_on_surface
