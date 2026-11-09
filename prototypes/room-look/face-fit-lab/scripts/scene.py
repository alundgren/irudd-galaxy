# Scene helpers shared by the render steps (bpy). Materials are looked up by name and
# created if missing; lights live in collections so a rig can be switched on alone.
import math
import os

import bpy
import numpy as np
from mathutils import Matrix, Vector

HEAD = "Bram_Head"
EYES = ("Bram_Eye_L", "Bram_Eye_R")
HEAD_C = Vector((0.0, -0.06, -0.02))      # face centre used for orbit views (m)


def principled(mat):
    return next(n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED")


def clay_material():
    m = bpy.data.materials.get("M_Clay")
    if m is None:
        m = bpy.data.materials.new("M_Clay")
        m.use_nodes = True
        b = principled(m)
        b.inputs["Base Color"].default_value = (0.42, 0.42, 0.42, 1)
        b.inputs["Roughness"].default_value = 0.65
    return m


def eye_clay_material():
    m = bpy.data.materials.get("M_Clay_Eye")
    if m is None:
        m = bpy.data.materials.new("M_Clay_Eye")
        m.use_nodes = True
        b = principled(m)
        b.inputs["Base Color"].default_value = (0.6, 0.6, 0.6, 1)
        b.inputs["Roughness"].default_value = 0.3
    return m


def set_materials(head_mat, eye_mat):
    """Point the head and eyes at the given materials (one slot each)."""
    for name, mat in ((HEAD, head_mat), (EYES[0], eye_mat), (EYES[1], eye_mat)):
        me = bpy.data.objects[name].data
        me.materials.clear()
        me.materials.append(mat)


def rig_collection(name):
    c = bpy.data.collections.get(name)
    if c is None:
        c = bpy.data.collections.new(name)
        bpy.context.scene.collection.children.link(c)
    return c


def area_light(coll, name, loc, target, power, color, size):
    o = bpy.data.objects.get(name)
    if o is None:
        o = bpy.data.objects.new(name, bpy.data.lights.new(name, "AREA"))
        coll.objects.link(o)
    o.data.energy = power
    o.data.color = color
    o.data.size = size
    o.location = loc
    d = (Vector(target) - Vector(loc)).normalized()
    o.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
    return o


def make_rigs(cam):
    """Lab rigs. 'clay': neutral studio key/fill for reading form. 'concept': warm key
    from the concept's light side (upper left of the fitted view = his right, above, in
    front) with a weak violet fill. 'swapped': the key moved to the opposite side and low,
    to test that the paint does not carry the concept's light."""
    cm = np.array(cam.matrix_world)
    R, C = cm[:3, :3], cm[:3, 3]
    right, up, back = R[:, 0], R[:, 1], R[:, 2]
    t = np.array(HEAD_C)

    def at(dx, dy, dz, dist=0.7):
        v = dx * right + dy * up + dz * back
        return Vector(t + v / np.linalg.norm(v) * dist)
    c = rig_collection("Rig_Clay")
    area_light(c, "Clay_Key", at(-0.6, 0.6, 1.0), t, 5, (1, 1, 1), 0.4)
    area_light(c, "Clay_Fill", at(0.9, 0.0, 0.6), t, 1.5, (1, 1, 1), 0.6)
    c = rig_collection("Rig_Concept")
    area_light(c, "Concept_Key", at(-0.9, 0.55, 0.7), t, 6, (1.0, 0.78, 0.55), 0.25)
    area_light(c, "Concept_Fill", at(1.0, -0.1, 0.4), t, 1.2, (0.62, 0.52, 1.0), 0.8)
    c = rig_collection("Rig_Swapped")
    area_light(c, "Swapped_Key", at(1.0, -0.45, 0.6), t, 6, (1.0, 0.78, 0.55), 0.25)
    area_light(c, "Swapped_Fill", at(-0.9, 0.3, 0.4), t, 1.2, (0.62, 0.52, 1.0), 0.8)


def use_rig(which):
    names = {"clay": "Rig_Clay", "concept": "Rig_Concept", "swapped": "Rig_Swapped"}
    for c in bpy.data.collections:
        if c.name.startswith("Rig_") or c.name.startswith("Bram_Rig_"):
            c.hide_render = c.name != names[which]
            for o in c.objects:
                if o.type == "LIGHT":
                    o.hide_render = c.name != names[which]


def world(color=(0.05, 0.045, 0.06), strength=1.0):
    w = bpy.data.worlds.get("W_Lab") or bpy.data.worlds.new("W_Lab")
    w.use_nodes = True
    bg = next(n for n in w.node_tree.nodes if n.type == "BACKGROUND")
    bg.inputs[0].default_value = (*color, 1)
    bg.inputs[1].default_value = strength
    bpy.context.scene.world = w


def orbit_camera(name, base, yaw_deg=0.0, pitch_deg=0.0, lens=None):
    """A copy of camera `base` orbited round the face centre: yaw about world Z, pitch
    about the camera's own right axis."""
    o = bpy.data.objects.get(name)
    if o is None:
        o = bpy.data.objects.new(name, bpy.data.cameras.new(name))
        bpy.context.scene.collection.objects.link(o)
    o.data.lens = lens or base.data.lens
    o.data.sensor_fit = "VERTICAL"
    o.data.sensor_height = 36.0
    o.data.clip_start = 0.01
    T = Matrix.Translation(HEAD_C)
    Rz = Matrix.Rotation(math.radians(yaw_deg), 4, "Z")
    right = base.matrix_world.to_3x3() @ Vector((1, 0, 0))
    Rp = Matrix.Rotation(math.radians(pitch_deg), 4, right)
    o.matrix_world = T @ Rz @ Rp @ T.inverted() @ base.matrix_world
    return o


def axis_camera(name, direction, dist=0.6, lens=85.0, up=(0, 0, 1)):
    """Orthographic-feel long-lens camera looking at the face centre from `direction`."""
    o = bpy.data.objects.get(name)
    if o is None:
        o = bpy.data.objects.new(name, bpy.data.cameras.new(name))
        bpy.context.scene.collection.objects.link(o)
    o.data.lens = lens
    o.data.sensor_fit = "VERTICAL"
    o.data.sensor_height = 36.0
    o.data.clip_start = 0.01
    d = Vector(direction).normalized()
    o.location = HEAD_C + d * dist
    o.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
    return o


def render(cam, path, w, h, engine=None, samples=32):
    sc = bpy.context.scene
    sc.camera = cam
    sc.render.resolution_x, sc.render.resolution_y = w, h
    sc.render.resolution_percentage = 100
    sc.render.use_stamp = False
    sc.render.film_transparent = False
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGB"
    sc.view_settings.view_transform = "Standard"
    sc.view_settings.look = "None"
    sc.view_settings.exposure = 0.0
    try:
        sc.eevee.taa_render_samples = samples
    except AttributeError:
        pass
    sc.render.filepath = path
    os.makedirs(os.path.dirname(path), exist_ok=True)
    bpy.ops.render.render(write_still=True)
    return path
