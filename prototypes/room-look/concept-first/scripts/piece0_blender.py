"""Piece 0 Blender side. Modes:
  id <out.png>                      object-ID render through the concept camera (Raw, no AA)
  views <palette.json> <concept.png> <outdir>   palette + projection renders, concept + orbit cameras
"""
import os, sys, json, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy
from mathutils import Vector
import cf, layout

args = cf.argv()
mode = args[0]
sc = cf.reset()
cf.set_engine(sc, "EEVEE")
cf.resolution(sc)
sc.render.dither_intensity = 0.0
try:
    sc.eevee.taa_render_samples = 8
except AttributeError:
    pass
world = bpy.data.worlds.new("World")
sc.world = world
world.color = (0, 0, 0)

ORDER = []


def mat_for_id(name):
    # one material per element category; categories get unique ids
    return None


if mode == "id":
    sc.view_settings.view_transform = "Raw"
    sc.render.filter_size = 0.0
    try:
        sc.eevee.taa_render_samples = 1
    except AttributeError:
        pass
    objs = layout.build(lambda n: None)
    names = sorted(objs)
    for i, n in enumerate(names, start=1):
        m = cf.flat_material("id_" + n, (i / 255.0, 0.0, 0.0))
        objs[n].data.materials.clear()
        objs[n].data.materials.append(m)
    cam = cf.concept_camera()
    cf.render(sc, cam, args[1])
    with open(os.path.splitext(args[1])[0] + ".json", "w") as f:
        json.dump({str(i): n for i, n in enumerate(names, start=1)}, f, indent=1)
    sys.exit(0)

# ---- views -------------------------------------------------------------------------------
palette = json.load(open(args[1]))
concept_path, outdir = args[2], args[3]
cf.standard_view(sc)
objs = layout.build(lambda n: None)
cam = cf.concept_camera()

# projector: an empty with the concept camera's transform
proj = bpy.data.objects.new("Concept projector", None)
sc.collection.objects.link(proj)
bpy.context.view_layer.update()
proj.matrix_world = cam.matrix_world.copy()
# visibility light at the projector: surfaces it cannot reach were never shown by the concept
vis = bpy.data.lights.new("Projector visibility", "POINT")
vis.energy, vis.shadow_soft_size = 2000, 0.0
vis_ob = bpy.data.objects.new("Projector visibility", vis)
sc.collection.objects.link(vis_ob)
vis_ob.location = cf.CAM_POS
UNSEEN = (1.0, 0.0, 1.0, 1)
img = bpy.data.images.load(concept_path)


def projection_material(name):
    m = bpy.data.materials.new(name)
    try:
        m.use_nodes = True
    except Exception:
        pass
    nt = m.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    tc = nt.nodes.new("ShaderNodeTexCoord")
    tc.object = proj
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(tc.outputs["Object"], sep.inputs[0])
    k = cf.CAM_FOCAL / cf.CAM_SENSOR
    aspect = cf.CONCEPT_W / cf.CONCEPT_H

    def math_node(op, a, b):
        n = nt.nodes.new("ShaderNodeMath")
        n.operation = op
        for i, v in enumerate((a, b)):
            if isinstance(v, (int, float)):
                n.inputs[i].default_value = v
            else:
                nt.links.new(v, n.inputs[i])
        return n.outputs[0]

    negz = math_node("MULTIPLY", sep.outputs["Z"], -1.0)
    xr = math_node("DIVIDE", sep.outputs["X"], negz)
    yr = math_node("DIVIDE", sep.outputs["Y"], negz)
    u = math_node("ADD", math_node("MULTIPLY", xr, k), 0.5)
    v = math_node("ADD", math_node("MULTIPLY", math_node("SUBTRACT", math_node("MULTIPLY", yr, k), cf.CAM_SHIFT_Y), aspect), 0.5)
    comb = nt.nodes.new("ShaderNodeCombineXYZ")
    nt.links.new(u, comb.inputs["X"])
    nt.links.new(v, comb.inputs["Y"])
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.extension = "CLIP"
    nt.links.new(comb.outputs[0], tex.inputs["Vector"])
    dif = nt.nodes.new("ShaderNodeBsdfDiffuse")
    s2r = nt.nodes.new("ShaderNodeShaderToRGB")
    nt.links.new(dif.outputs[0], s2r.inputs[0])
    bw = nt.nodes.new("ShaderNodeRGBToBW")
    nt.links.new(s2r.outputs["Color"], bw.inputs[0])
    # direct light from the 2000 W projector light gives ~5-20 here; bounce light stays far below 0.5
    seen = math_node("GREATER_THAN", bw.outputs[0], 0.5)
    mix = nt.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    ca, cb = [i for i in mix.inputs if i.type == "RGBA"][:2]
    ca.default_value = UNSEEN
    nt.links.new(seen, mix.inputs[0])
    nt.links.new(tex.outputs["Color"], cb)
    em = nt.nodes.new("ShaderNodeEmission")
    nt.links.new([o for o in mix.outputs if o.type == "RGBA"][0], em.inputs["Color"])
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(em.outputs[0], out.inputs["Surface"])
    return m


def lit_material(name, srgb):
    m = bpy.data.materials.new(name)
    try:
        m.use_nodes = True
    except Exception:
        pass
    b = cf.find(m.node_tree, "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = (*cf.lin([c * 255 for c in srgb]), 1)
    b.inputs["Roughness"].default_value = 0.8
    return m


pm = projection_material("concept projection")
flat = {n: cf.flat_material("flat_" + n, cf.lin([c * 255 for c in palette.get(n, (0.5, 0.5, 0.5))])) for n in objs}
lit = {n: lit_material("lit_" + n, palette.get(n, (0.5, 0.5, 0.5))) for n in objs}


def assign(table):
    for n, ob in objs.items():
        ob.data.materials.clear()
        ob.data.materials.append(table[n] if isinstance(table, dict) else table)


# simple light rig for the lit check: lamp bulb + cool fill from the porthole
bulb = bpy.data.lights.new("Bulb", "POINT")
bulb.energy, bulb.color, bulb.shadow_soft_size = 60, (1.0, 0.75, 0.45), 0.05
bo = bpy.data.objects.new("Bulb", bulb)
sc.collection.objects.link(bo)
bo.location = objs["lamp_shade"].location + Vector((0, 0, -0.08))
fill = bpy.data.lights.new("Porthole", "AREA")
fill.energy, fill.color, fill.size = 80, (0.6, 0.5, 1.0), 0.6
fo = bpy.data.objects.new("Porthole", fill)
sc.collection.objects.link(fo)
fo.location = layout.porthole_center() + Vector((0.3, -0.3, 0))
fo.rotation_euler = (0, math.radians(90), math.radians(40))
world.color = (0.05, 0.04, 0.07)

views = [("concept", cam), ("yaw-25", cf.orbit_camera("yaw-25", -25)), ("yaw+25", cf.orbit_camera("yaw+25", 25)),
         ("yaw+50", cf.orbit_camera("yaw+50", 50)), ("high", cf.orbit_camera("high", 0, 25))]
for label, table in (("projected", pm), ("palette", flat), ("lit", lit)):
    assign(table)
    projected = label == "projected"
    vis_ob.hide_render = not projected
    bo.hide_render = fo.hide_render = projected
    world.color = (0, 0, 0) if projected else (0.05, 0.04, 0.07)
    for vn, vc in views:
        cf.render(sc, vc, os.path.join(outdir, f"{label}-{vn}.png"))
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(outdir, "layout.blend"))
