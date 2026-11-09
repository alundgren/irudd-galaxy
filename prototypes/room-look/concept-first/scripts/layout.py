"""Piece 0: the room blockout, placed from concept pixel positions.

Every element is written in concept/room.png pixel coordinates (read off concept/room-grid.png)
plus one depth choice. The helpers turn pixels into metres through the matched camera, so the
blockout overlays the concept by construction; the overlay render checks what the guesses got wrong.
"""
import math
import bpy
from mathutils import Vector
import cf

DESK_Z = 1.05


def ray_dir(x, y):
    cx = cf.CONCEPT_W / 2.0
    cy = cf.CONCEPT_H / 2.0 + cf.CAM_SHIFT_Y * cf.CONCEPT_W
    return Vector(((x - cx) / cf.F_PX, 1.0, -(y - cy) / cf.F_PX))


def px_on_z(x, y, z):
    d = ray_dir(x, y)
    t = (z - cf.CAM_POS.z) / d.z
    return cf.CAM_POS + d * t


def px_on_y(x, y, plane_y):
    return cf.px_to_world_on_plane(x, y, plane_y)


def wall_box(name, x0, x1, y0, y1, plane_y, depth, mat=None, bevel=0.0):
    """Front face = pixel rectangle projected onto Y = plane_y; box extends `depth` toward the wall."""
    a = px_on_y(x0, y1, plane_y)
    b = px_on_y(x1, y0, plane_y)
    return cf.box(name, a.x, b.x, plane_y, plane_y + depth, a.z, b.z, mat, bevel)


def desk_item(name, x0, x1, y_top, y_base, depth, base_z=DESK_Z, mat=None, bevel=0.0):
    """Item standing on a horizontal surface: the base row finds how far back it stands."""
    front = px_on_z((x0 + x1) / 2, y_base, base_z)
    a = px_on_y(x0, y_base, front.y)
    b = px_on_y(x1, y_top, front.y)
    return cf.box(name, a.x, b.x, front.y, front.y + depth, base_z, b.z, mat, bevel)


def desk_cylinder(name, x0, x1, y_top, y_base, base_z=DESK_Z, mat=None):
    front = px_on_z((x0 + x1) / 2, y_base, base_z)
    a = px_on_y(x0, y_base, front.y)
    b = px_on_y(x1, y_top, front.y)
    r = (b.x - a.x) / 2
    h = b.z - base_z
    return cf.cylinder(name, Vector(((a.x + b.x) / 2, front.y + r, base_z + h / 2)), r, h, "Z", 40, mat)


def flat_on_desk(name, x0, x1, y_back, y_front, height, base_z=DESK_Z, mat=None):
    p0 = px_on_z(x0, y_front, base_z)
    p1 = px_on_z(x1, y_back, base_z)
    return cf.box(name, p0.x, p1.x, p0.y, p1.y, base_z, base_z + height, mat)


def rod(name, p0, p1, r, mat=None):
    p0, p1 = Vector(p0), Vector(p1)
    mid = (p0 + p1) / 2
    ob = cf.cylinder(name, mid, r, (p1 - p0).length, "Z", 24, mat)
    ob.rotation_euler = (p1 - p0).to_track_quat("Z", "Y").to_euler()
    return ob


def blob(name, cx, cy, w_px, h_px, plane_y, depth_m, mat=None):
    """Ellipsoid whose silhouette matches a pixel ellipse on plane Y = plane_y."""
    c = px_on_y(cx, cy, plane_y)
    s = 1.0 / cf.px_per_m(plane_y)
    return cf.uv_sphere(name, c, (w_px * s / 2, depth_m / 2, h_px * s / 2), mat)


# Left wall: turns toward the viewer at the teal pillar (concept x ~ 350) at this angle.
LEFT_CORNER_X = -0.42
LEFT_ANGLE = math.radians(40)


def left_wall_point(t, z):
    return Vector((LEFT_CORNER_X - t * math.cos(LEFT_ANGLE), -t * math.sin(LEFT_ANGLE), z))


PORTHOLE_R = 0.31   # opening radius: 163 px tall in the concept at ~2.0 m from the camera


def porthole_center():
    return left_wall_point(0.428, 1.70)  # on the left wall's room-side face


def LAMP_WALL_MOUNT():
    return px_on_y(395, 150, -0.08)


# Element table: name -> category. Categories pick materials in the piece scripts.
def build(mat_for):
    """mat_for(name) -> material. Returns {name: object}."""
    o = {}

    def add(ob):
        o[ob.name] = ob
        return ob

    # shell
    add(cf.box("wall_back", LEFT_CORNER_X, 2.0, 0.0, 0.1, 0.0, 3.2, mat_for("wall_back")))
    # thickness behind the wall line, so the room-side face is the line itself
    lw = add(cf.box("wall_left", -2.2, 0.0, 0.0, 0.1, 0.0, 3.2, mat_for("wall_left")))
    lw.location = (LEFT_CORNER_X, 0, 0)
    lw.rotation_euler = (0, 0, LEFT_ANGLE)  # +X edge of the box stays at the corner
    add(cf.box("floor", -3, 3, -3, 0.1, -0.05, 0.0, mat_for("floor")))

    # porthole on the left wall: a real hole, an open tube going out, a rounded bezel on the room side
    pc = porthole_center()
    outward = Vector((-math.sin(LEFT_ANGLE), math.cos(LEFT_ANGLE), 0))
    cutter = cf.cylinder("porthole_cutter", pc, PORTHOLE_R, 1.0, "Y", 64)
    cutter.rotation_euler = (0, 0, LEFT_ANGLE)
    mod = lw.modifiers.new("hole", "BOOLEAN")
    mod.object, mod.operation = cutter, "DIFFERENCE"
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    holed = bpy.data.meshes.new_from_object(lw.evaluated_get(dg))
    lw.modifiers.remove(mod)
    lw.data = holed
    bpy.data.objects.remove(cutter)
    tube = add(cf.cylinder("porthole_tube", pc + outward * 0.175, PORTHOLE_R, 0.35, "Y", 64, mat_for("porthole_tube"), cap=False))
    tube.rotation_euler = (0, 0, LEFT_ANGLE)
    for p in tube.data.polygons:
        p.use_smooth = True
    tube.data.flip_normals()                     # seen from inside
    import bmesh
    me = bpy.data.meshes.new("porthole_bezel")
    bm = bmesh.new()  # torus from parametric points
    R, r, nU, nV = PORTHOLE_R + 0.055, 0.055, 64, 16
    verts = []
    for i in range(nU):
        a = 2 * math.pi * i / nU
        for j in range(nV):
            b = 2 * math.pi * j / nV
            # ring in the local XZ plane, tube cross-section flattened along local Y (depth)
            x = (R + r * math.cos(b)) * math.cos(a)
            z = (R + r * math.cos(b)) * math.sin(a)
            y = -0.6 * r * math.sin(b)
            verts.append(bm.verts.new((x, y, z)))
    for i in range(nU):
        for j in range(nV):
            a0, a1 = i * nV + j, ((i + 1) % nU) * nV + j
            b0, b1 = i * nV + (j + 1) % nV, ((i + 1) % nU) * nV + (j + 1) % nV
            bm.faces.new((verts[a0], verts[a1], verts[b1], verts[b0]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(me)
    bm.free()
    bez = add(cf.link(bpy.data.objects.new("porthole_bezel", me)))
    bez.location = pc
    bez.rotation_euler = (0, 0, LEFT_ANGLE)
    me.materials.append(mat_for("porthole_bezel"))
    for p in me.polygons:
        p.use_smooth = True

    add(wall_box("pillar_teal", 330, 388, 0, 640, -0.06, 0.06, mat_for("pillar_teal")))
    add(wall_box("arches", 495, 1000, 0, 245, -0.04, 0.04, mat_for("arches")))

    # arrivals board
    add(wall_box("board_frame", 530, 915, 235, 645, -0.07, 0.07, mat_for("board_frame")))
    rows = [(278, 332), (338, 390), (396, 446), (453, 500), (506, 548), (555, 598)]
    cols = [(555, 690), (695, 790), (795, 885)]
    for i, (r0, r1) in enumerate(rows):
        for j, (c0, c1) in enumerate(cols):
            add(wall_box(f"board_slot_{i}{j}", c0, c1, r0, r1, -0.085, 0.015, mat_for("board_slot")))

    # lamp
    shade_c = px_on_y(579, 175, -0.45)
    s = 1 / cf.px_per_m(-0.45)
    r_bot = 97 * s
    h = (222 - 128) * s
    import bmesh
    me = bpy.data.meshes.new("lamp_shade")
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=r_bot, radius2=r_bot * 0.35, depth=h)
    bm.to_mesh(me)
    bm.free()
    shade = add(cf.link(bpy.data.objects.new("lamp_shade", me)))
    shade.location = shade_c
    me.materials.append(mat_for("lamp_shade"))
    for p in me.polygons:
        p.use_smooth = True
    arm_wall = LAMP_WALL_MOUNT()
    arm_elbow = px_on_y(545, 60, -0.30)
    add(rod("lamp_arm_a", arm_wall, arm_elbow, 0.022, mat_for("lamp_arm")))
    add(rod("lamp_arm_b", arm_elbow, shade_c + Vector((0, 0, h / 2)), 0.022, mat_for("lamp_arm")))

    # desk (counter)
    add(cf.box("desk_top", -1.4, 1.4, -0.99, -0.52, DESK_Z - 0.05, DESK_Z, mat_for("desk_top")))
    add(cf.box("desk_front", -1.4, 1.4, -1.0, -0.96, 0.0, DESK_Z - 0.05, mat_for("desk_front")))

    # desk props
    add(flat_on_desk("book", 290, 770, 705, 790, 0.035, mat=mat_for("book")))
    add(desk_item("sign", 725, 1000, 705, 792, 0.08, mat=mat_for("sign")))
    add(desk_cylinder("mug", 765, 842, 637, 726, mat=mat_for("mug")))
    add(desk_item("rack", 905, 1075, 612, 745, 0.12, mat=mat_for("rack")))
    add(desk_item("case_low", 12, 242, 745, 802, 0.20, mat=mat_for("case")))
    add(desk_item("case_high", 18, 236, 688, 745, 0.20, base_z=DESK_Z + 0.07, mat=mat_for("case")))

    # plant on the ledge under the porthole
    add(desk_cylinder("plant_pot", 0, 82, 602, 700, mat=mat_for("plant_pot")))
    add(blob("plant_leaves", 70, 500, 150, 280, -0.80, 0.25, mat_for("plant_leaves")))

    # Bram (seated behind the desk, about 0.35 m in front of the wall). Foreground blockouts are drawn
    # ~10% larger than the painted silhouette: the matte trims the excess, but paint outside the
    # blockout is lost (his epaulette was).
    BY = -0.38
    add(blob("bram_head", 458, 392, 150, 190, BY, 0.24, mat_for("bram_skin")))
    add(blob("bram_cap", 452, 316, 220, 106, BY + 0.01, 0.30, mat_for("bram_cap")))
    add(blob("bram_torso", 425, 600, 550, 410, BY + 0.05, 0.36, mat_for("bram_jacket")))
    sh_l, el_l, hd_l = px_on_y(250, 500, BY), px_on_y(300, 690, BY - 0.25), px_on_y(470, 725, -0.62)
    add(rod("bram_arm_l_up", sh_l, el_l, 0.075, mat_for("bram_jacket")))
    add(rod("bram_arm_l_lo", el_l, hd_l, 0.065, mat_for("bram_jacket")))
    sh_r, el_r, hd_r = px_on_y(600, 520, BY), px_on_y(610, 655, -0.62), px_on_y(505, 470, BY - 0.12)
    add(rod("bram_arm_r_up", sh_r, el_r, 0.075, mat_for("bram_jacket")))
    add(rod("bram_arm_r_lo", el_r, hd_r, 0.06, mat_for("bram_jacket")))
    add(wall_box("chair_back", 105, 168, 555, 700, -0.20, 0.08, mat_for("chair")))
    return o
