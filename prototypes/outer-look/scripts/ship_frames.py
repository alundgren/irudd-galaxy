"""Pre-render the ship's turn sheet: a proxy matched to the concept ship, toon-lit, inked.

    blender -b --factory-startup --python scripts/ship_frames.py -- <out folder> [headings=64] [px=256]

Writes <out>/base/NN.png and <out>/pod/NN.png (the same ship with a fitted mining pod),
one frame per heading, heading 0 = facing +X (screen right), counter-clockwise. The camera
is orthographic, 40 degrees above the play plane, like the station plate's camera. The sun
stays fixed in the world, so the light holds while the ship turns.

This is the stand-in for a painted turn sheet. Its job in the prototype is to show what
any turn sheet does in motion; its look is the weak part and is scored as such.
"""
import math
import os
import sys

import bpy

argv = sys.argv[sys.argv.index("--") + 1:]
OUT = argv[0]
N = int(argv[1]) if len(argv) > 1 else 64
PX = int(argv[2]) if len(argv) > 2 else 256

# colours sampled from the concept ship (palette.py boxes), sRGB
PAL = {
    "hull": "#d4503c",
    "canopy": "#2f6f78",
    "fin": "#2f8a86",
    "wing": "#f0b43a",
    "nozzle": "#8c8590",
    "trim": "#e9c06a",
    "dark": "#3a2a3a",
    "glow": "#ffb04a",
}
INK = "#1e0f16"
SHADOW = (0.24, 0.25, 0.38)  # measured shadow multiplier from the concepts (NEXT.md 1.3)


def srgb(h):
    h = h.lstrip("#")
    c = [int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)]
    return [x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c] + [1.0]


bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
for eng in ("BLENDER_EEVEE", "BLENDER_EEVEE_NEXT"):
    try:
        scene.render.engine = eng
        break
    except TypeError:
        pass
scene.render.resolution_x = scene.render.resolution_y = PX
scene.render.film_transparent = True
scene.view_settings.view_transform = "Standard"
scene.render.image_settings.file_format = "PNG"
scene.render.image_settings.color_mode = "RGBA"
try:
    scene.eevee.taa_render_samples = 16
except AttributeError:
    pass


def toon(name, hexcol, glow=0.0, marks=True):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    if glow:
        em = nt.nodes.new("ShaderNodeEmission")
        em.inputs["Color"].default_value = srgb(hexcol)
        em.inputs["Strength"].default_value = glow
        nt.links.new(em.outputs[0], out.inputs[0])
        return m
    dif = nt.nodes.new("ShaderNodeBsdfDiffuse")
    s2r = nt.nodes.new("ShaderNodeShaderToRGB")
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.interpolation = "CONSTANT"
    base = srgb(hexcol)
    shadow = [base[i] * SHADOW[i] for i in range(3)] + [1]
    mid = [base[i] * (0.55 + 0.45 * SHADOW[i]) for i in range(3)] + [1]
    lit = [min(1.0, base[i] * 1.0) for i in range(3)] + [1]
    rim = [min(1.0, base[i] * 1.25 + 0.04) for i in range(3)] + [1]
    els = ramp.color_ramp.elements
    els[0].position, els[0].color = 0.0, shadow
    els[1].position, els[1].color = 0.08, mid
    e = els.new(0.22)
    e.color = lit
    e = els.new(0.6)
    e.color = rim
    nt.links.new(dif.outputs[0], s2r.inputs[0])
    nt.links.new(s2r.outputs["Color"], ramp.inputs["Fac"])
    col = ramp.outputs["Color"]
    if marks:
        # fine drawn marks: panel seams and rivets as dark lines in object space
        tc = nt.nodes.new("ShaderNodeTexCoord")
        brick = nt.nodes.new("ShaderNodeTexBrick")
        brick.inputs["Scale"].default_value = 4.0
        brick.inputs["Mortar Size"].default_value = 0.02
        brick.inputs["Color1"].default_value = (1, 1, 1, 1)
        brick.inputs["Color2"].default_value = (0.93, 0.9, 0.9, 1)
        brick.inputs["Mortar"].default_value = (0.35, 0.22, 0.25, 1)
        nt.links.new(tc.outputs["Object"], brick.inputs["Vector"])
        noise = nt.nodes.new("ShaderNodeTexNoise")
        noise.inputs["Scale"].default_value = 14.0
        nramp = nt.nodes.new("ShaderNodeValToRGB")
        nramp.color_ramp.elements[0].position = 0.45
        nramp.color_ramp.elements[0].color = (0.8, 0.72, 0.72, 1)
        nramp.color_ramp.elements[1].position = 0.62
        nt.links.new(tc.outputs["Object"], noise.inputs["Vector"])
        nt.links.new(noise.outputs["Fac"], nramp.inputs["Fac"])
        mul = nt.nodes.new("ShaderNodeMix")
        mul.data_type = "RGBA"
        mul.blend_type = "MULTIPLY"
        mul.inputs["Factor"].default_value = 1.0
        nt.links.new(col, mul.inputs[6])
        nt.links.new(brick.outputs["Color"], mul.inputs[7])
        mul2 = nt.nodes.new("ShaderNodeMix")
        mul2.data_type = "RGBA"
        mul2.blend_type = "MULTIPLY"
        mul2.inputs["Factor"].default_value = 1.0
        nt.links.new(mul.outputs[2], mul2.inputs[6])
        nt.links.new(nramp.outputs["Color"], mul2.inputs[7])
        col = mul2.outputs[2]
    em = nt.nodes.new("ShaderNodeEmission")
    nt.links.new(col, em.inputs["Color"])
    nt.links.new(em.outputs[0], out.inputs[0])
    return m


def ink_mat():
    m = bpy.data.materials.new("ink")
    m.use_nodes = True
    nt = m.node_tree
    em = nt.nodes.new("ShaderNodeEmission")
    em.inputs["Color"].default_value = srgb(INK)
    out = next(n for n in nt.nodes if n.type == "OUTPUT_MATERIAL")
    nt.links.new(em.outputs[0], out.inputs[0])
    m.use_backface_culling = True
    return m


INKM = ink_mat()
MATS = {k: toon(k, v, marks=k in ("hull", "wing", "fin")) for k, v in PAL.items() if k != "glow"}
MATS["glow"] = toon("glow", PAL["glow"], glow=3.0)
MATS["canopy_glass"] = toon("canopy_glass", PAL["canopy"], marks=False)

root = bpy.data.objects.new("ship", None)
scene.collection.objects.link(root)


def add(obj, mat, ink=0.035):
    obj.data.materials.append(MATS[mat])
    obj.parent = root
    if ink:
        obj.data.materials.append(INKM)
        sol = obj.modifiers.new("ink", "SOLIDIFY")
        sol.thickness = ink
        sol.offset = 1.0
        sol.use_flip_normals = True
        sol.material_offset = 1
    bpy.ops.object.shade_smooth() if False else None
    for p in obj.data.polygons:
        p.use_smooth = True
    return obj


def sphere(name, loc, scale, mat, rot=(0, 0, 0), seg=32, ink=0.035):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=seg, ring_count=seg // 2, location=loc, rotation=rot)
    o = bpy.context.active_object
    o.name = name
    o.scale = scale
    bpy.ops.object.transform_apply(scale=True)
    return add(o, mat, ink)


def cyl(name, loc, r, depth, mat, rot=(0, math.pi / 2, 0), ink=0.03, verts=24):
    bpy.ops.mesh.primitive_cylinder_add(vertices=verts, radius=r, depth=depth, location=loc, rotation=rot)
    o = bpy.context.active_object
    o.name = name
    return add(o, mat, ink)


def box(name, loc, scale, mat, rot=(0, 0, 0), ink=0.03):
    bpy.ops.mesh.primitive_cube_add(location=loc, rotation=rot)
    o = bpy.context.active_object
    o.name = name
    o.scale = scale
    bpy.ops.object.transform_apply(scale=True)
    bev = o.modifiers.new("bev", "BEVEL")
    bev.width = 0.04
    bev.segments = 2
    return add(o, mat, ink)


# the ship, nose toward +X, about 2.6 units long
sphere("hull", (0.15, 0, 0), (1.25, 0.5, 0.48), "hull")
sphere("canopy", (0.62, 0, 0.2), (0.55, 0.36, 0.3), "canopy_glass", ink=0.025)
box("stripe", (0.0, 0, 0.42), (0.7, 0.07, 0.06), "trim", ink=0.015)
for s in (-1, 1):
    cyl("engine%d" % s, (-1.05, 0.26 * s, -0.02), 0.24, 0.6, "nozzle")
    cyl("ring%d" % s, (-1.32, 0.26 * s, -0.02), 0.2, 0.08, "dark", ink=0.0)
    cyl("flame%d" % s, (-1.38, 0.26 * s, -0.02), 0.15, 0.04, "glow", ink=0.0)
    box("wing%d" % s, (-0.35, 0.72 * s, -0.12), (0.5, 0.36, 0.05), "wing", rot=(0, 0, -0.35 * s))
    sphere("mark%d" % s, (-0.35, 0.8 * s, -0.06), (0.14, 0.14, 0.03), "dark", ink=0.0, seg=16)
box("fin", (-0.7, 0, 0.62), (0.42, 0.04, 0.32), "fin", rot=(0, -0.45, 0))
box("fintip", (-0.82, 0, 0.92), (0.3, 0.05, 0.04), "wing", rot=(0, -0.2, 0), ink=0.015)

pod = []
for s in (-1, 1):
    pod.append(cyl("pod%d" % s, (-0.2, 0.95 * s, 0.05), 0.13, 0.85, "trim"))
    pod.append(sphere("lens%d" % s, (0.25, 0.95 * s, 0.05), (0.09, 0.09, 0.09), "glow", ink=0.0, seg=16))

# camera: orthographic, 45 degrees above the plane, looking north (screen up = world +Y)
cam_data = bpy.data.cameras.new("cam")
cam_data.type = "ORTHO"
cam_data.ortho_scale = 3.6
cam = bpy.data.objects.new("cam", cam_data)
scene.collection.objects.link(cam)
el = math.radians(40)
cam.location = (0, -10 * math.cos(el), 10 * math.sin(el))
cam.rotation_euler = (math.pi / 2 - el, 0, 0)
scene.camera = cam

sun_data = bpy.data.lights.new("sun", "SUN")
sun_data.energy = 6.0
sun = bpy.data.objects.new("sun", sun_data)
scene.collection.objects.link(sun)
sun.rotation_euler = (math.radians(40), math.radians(-35), math.radians(-30))  # from upper left, fixed in the world

for variant, show_pod in (("base", False), ("pod", True)):
    for o in pod:
        o.hide_render = not show_pod
    os.makedirs(os.path.join(OUT, variant), exist_ok=True)
    for i in range(N):
        root.rotation_euler = (0, 0, 2 * math.pi * i / N)
        scene.render.filepath = os.path.join(OUT, variant, "%02d.png" % i)
        bpy.ops.render.render(write_still=True)
    print("rendered", variant, N)
