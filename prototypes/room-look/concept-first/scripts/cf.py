"""Concept-first helpers shared by every piece script.

Run inside a background Blender:  blender -b --factory-startup --disable-autoexec --python piece.py -- args
All lengths are metres. World: X right, Y away from the viewer (into the back wall), Z up.
The back wall is the plane Y = 0. The concept camera stands at -Y looking +Y.
"""
import bpy, math, os, sys
from mathutils import Vector, Matrix

# ---- concept camera (piece 0 match; see LOG.md for the derivation) ----------------------
CONCEPT_W, CONCEPT_H = 1083, 893        # concept/room.png size in pixels
CAM_FOCAL = 35.0                        # mm, sensor 36 mm wide, horizontal fit
CAM_SENSOR = 36.0
CAM_POS = Vector((0.0, -2.29, 1.58))    # standing eye 1.58 m, 2.29 m from the back wall
CAM_SHIFT_Y = -0.077                    # puts the horizon at concept row 363
F_PX = CAM_FOCAL / CAM_SENSOR * CONCEPT_W  # focal length in concept pixels (~1053)


def argv():
    return sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.unit_settings.system = "METRIC"
    return sc


def set_engine(sc, want):
    """want: 'EEVEE' or 'CYCLES'. Engine ids differ between versions, so try candidates."""
    cands = ["BLENDER_EEVEE", "BLENDER_EEVEE_NEXT"] if want == "EEVEE" else ["CYCLES"]
    for c in cands:
        try:
            sc.render.engine = c
            return c
        except TypeError:
            pass
    raise RuntimeError("no engine for " + want)


def use_gpu_cycles(sc, samples=32):
    set_engine(sc, "CYCLES")
    try:
        prefs = bpy.context.preferences.addons["cycles"].preferences
        prefs.compute_device_type = "OPTIX"
        prefs.get_devices()
        for d in prefs.devices:
            d.use = d.type == "OPTIX"
        sc.cycles.device = "GPU"
    except Exception as e:
        print("GPU unavailable, CPU instead:", e)
    sc.cycles.samples = samples
    sc.cycles.use_denoising = True


def standard_view(sc):
    # 'Standard' keeps concept-sampled colours as they are; AgX/Filmic would shift them.
    sc.view_settings.view_transform = "Standard"
    sc.view_settings.look = "None"


def resolution(sc, w=CONCEPT_W, h=CONCEPT_H, pct=100):
    sc.render.resolution_x, sc.render.resolution_y = w, h
    sc.render.resolution_percentage = pct


def concept_camera(name="Concept camera"):
    cam = bpy.data.cameras.new(name)
    cam.lens = CAM_FOCAL
    cam.sensor_width = CAM_SENSOR
    cam.sensor_fit = "HORIZONTAL"
    cam.shift_y = CAM_SHIFT_Y
    cam.clip_start, cam.clip_end = 0.05, 100
    ob = bpy.data.objects.new(name, cam)
    bpy.context.scene.collection.objects.link(ob)
    ob.location = CAM_POS
    ob.rotation_euler = (math.radians(90), 0, 0)
    return ob


def orbit_camera(name, yaw_deg, pitch_deg=0.0, target=Vector((0.2, -0.9, 1.5)), dist=None, lens=35.0):
    """Camera on a circle around `target`, yaw 0 = concept direction, positive yaw = to the right."""
    if dist is None:
        dist = (CAM_POS - target).length
    cam = bpy.data.cameras.new(name)
    cam.lens = lens
    cam.sensor_width = CAM_SENSOR
    ob = bpy.data.objects.new(name, cam)
    bpy.context.scene.collection.objects.link(ob)
    y, p = math.radians(yaw_deg), math.radians(pitch_deg)
    off = Vector((math.sin(y) * math.cos(p), -math.cos(y) * math.cos(p), math.sin(p))) * dist
    ob.location = target + off
    d = (target - ob.location).normalized()
    ob.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
    return ob


def px_to_world_on_plane(x_px, y_px, plane_y):
    """Concept pixel (crop coords) -> world point on the plane Y = plane_y."""
    depth = plane_y - CAM_POS.y
    cx = CONCEPT_W / 2.0
    cy = CONCEPT_H / 2.0 + CAM_SHIFT_Y * CONCEPT_W
    X = CAM_POS.x + (x_px - cx) * depth / F_PX
    Z = CAM_POS.z - (y_px - cy) * depth / F_PX
    return Vector((X, plane_y, Z))


def px_per_m(plane_y):
    return F_PX / (plane_y - CAM_POS.y)


# ---- geometry -----------------------------------------------------------------------------
def link(ob, coll=None):
    (coll or bpy.context.scene.collection).objects.link(ob)
    return ob


def box(name, x0, x1, y0, y1, z0, z1, mat=None, bevel=0.0):
    import bmesh
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for v in bm.verts:
        v.co.x = x0 if v.co.x < 0 else x1
        v.co.y = y0 if v.co.y < 0 else y1
        v.co.z = z0 if v.co.z < 0 else z1
    bm.to_mesh(me)
    bm.free()
    ob = link(bpy.data.objects.new(name, me))
    if mat:
        me.materials.append(mat)
    if bevel > 0:
        m = ob.modifiers.new("Bevel", "BEVEL")
        m.width, m.segments = bevel, 2
    return ob


def cylinder(name, center, radius, depth, axis="Y", verts=48, mat=None, cap=True):
    import bmesh
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=cap, segments=verts, radius1=radius, radius2=radius, depth=depth)
    rot = {"X": Matrix.Rotation(math.radians(90), 4, "Y"),
           "Y": Matrix.Rotation(math.radians(90), 4, "X"),
           "Z": Matrix.Identity(4)}[axis]
    bmesh.ops.transform(bm, matrix=rot, verts=bm.verts)
    bm.to_mesh(me)
    bm.free()
    ob = link(bpy.data.objects.new(name, me))
    ob.location = center
    if mat:
        me.materials.append(mat)
    return ob


def uv_sphere(name, center, scale, mat=None, segs=48, rings=24):
    import bmesh
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=segs, v_segments=rings, radius=1.0)
    bm.to_mesh(me)
    bm.free()
    ob = link(bpy.data.objects.new(name, me))
    ob.location = center
    ob.scale = scale
    if mat:
        me.materials.append(mat)
    for p in me.polygons:
        p.use_smooth = True
    return ob


# ---- materials ----------------------------------------------------------------------------
def node(nt, type_name):
    return nt.nodes.new(type_name)


def find(nt, typ):
    return next(n for n in nt.nodes if n.type == typ)


def flat_material(name, rgb):
    """Unlit flat colour (emission). Used for ID/overlay renders."""
    m = bpy.data.materials.new(name)
    try:
        m.use_nodes = True
    except Exception:
        pass
    nt = m.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = node(nt, "ShaderNodeOutputMaterial")
    em = node(nt, "ShaderNodeEmission")
    em.inputs["Color"].default_value = (*rgb, 1)
    nt.links.new(em.outputs[0], out.inputs["Surface"])
    m.diffuse_color = (*rgb, 1)
    return m


def srgb_to_linear(c):
    c = c / 255.0 if c > 1 else c
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def lin(rgb255):
    return tuple(srgb_to_linear(c) for c in rgb255)


def math_node(nt, op, a, b):
    n = nt.nodes.new("ShaderNodeMath")
    n.operation = op
    for i, v in enumerate((a, b)):
        if isinstance(v, (int, float)):
            n.inputs[i].default_value = v
        else:
            nt.links.new(v, n.inputs[i])
    return n.outputs[0]


def projector_empty(cam_ob):
    """An empty at the concept camera's pose. Its object space is the camera's space."""
    bpy.context.view_layer.update()
    e = bpy.data.objects.new("Concept projector", None)
    bpy.context.scene.collection.objects.link(e)
    e.matrix_world = cam_ob.matrix_world.copy()
    return e


def projected_uv(nt, proj):
    """Shader nodes: where this surface point lands in the concept image (0..1, Y up)."""
    tc = nt.nodes.new("ShaderNodeTexCoord")
    tc.object = proj
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(tc.outputs["Object"], sep.inputs[0])
    k = CAM_FOCAL / CAM_SENSOR
    aspect = CONCEPT_W / CONCEPT_H
    negz = math_node(nt, "MULTIPLY", sep.outputs["Z"], -1.0)
    xr = math_node(nt, "DIVIDE", sep.outputs["X"], negz)
    yr = math_node(nt, "DIVIDE", sep.outputs["Y"], negz)
    u = math_node(nt, "ADD", math_node(nt, "MULTIPLY", xr, k), 0.5)
    v = math_node(nt, "ADD", math_node(nt, "MULTIPLY", math_node(nt, "SUBTRACT", math_node(nt, "MULTIPLY", yr, k), CAM_SHIFT_Y), aspect), 0.5)
    comb = nt.nodes.new("ShaderNodeCombineXYZ")
    nt.links.new(u, comb.inputs["X"])
    nt.links.new(v, comb.inputs["Y"])
    return comb.outputs[0]


def new_material(name):
    m = bpy.data.materials.new(name)
    try:
        m.use_nodes = True
    except Exception:
        pass
    nt = m.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    return m, nt, out


def projection_emit_material(name, proj, img):
    m, nt, out = new_material(name)
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.extension = "CLIP"
    nt.links.new(projected_uv(nt, proj), tex.inputs["Vector"])
    em = nt.nodes.new("ShaderNodeEmission")
    nt.links.new(tex.outputs["Color"], em.inputs["Color"])
    nt.links.new(em.outputs[0], out.inputs["Surface"])
    return m


def render(sc, cam, path):
    sc.camera = cam
    sc.render.filepath = path
    sc.render.image_settings.file_format = "PNG"
    bpy.ops.render.render(write_still=True)
