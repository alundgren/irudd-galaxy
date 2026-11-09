# Step 4. Materials and ink for the arms, saved into face_fit.blend.
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/build_look.py
#   M_Face_Cel / M_Face_Cel_C  painted layer x cel band, emitted (so the bands are the only
#                              lighting): 3 hard bands from the scene's lights via
#                              Shader to RGB (EEVEE), shadow band warm violet (direction.md 1.1).
#   M_Face_Raw                 the raw projected layer, unlit, for diagnostics.
#   M_Eye_Cel                  the Bram lab's eye image, toned down, under the same bands.
#   M_Ink                      plum #2a1a2e, back faces culled: the outline shell.
#   Ink_Hull                   Solidify on the head (1.5 mm, flipped, material slot 2).
import os
import sys

import bpy

PLUM_LIN = (0.0232, 0.0103, 0.0273, 1)          # #2a1a2e in linear
BANDS = [(0.0, (0.47, 0.36, 0.56)),             # shadow: warm violet
         (0.16, (0.80, 0.72, 0.80)),            # mid
         (0.42, (1.00, 0.97, 0.92))]            # light


def node(nt, kind, x, y, **kw):
    n = nt.nodes.new(kind)
    n.location = (x, y)
    for k, v in kw.items():
        setattr(n, k, v)
    return n


def cel_material(name, image, uv="UVMap_HBM", tint=None):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    out = node(nt, "ShaderNodeOutputMaterial", 900, 0)
    em = node(nt, "ShaderNodeEmission", 700, 0)
    mul = node(nt, "ShaderNodeMix", 500, 0, data_type="RGBA", blend_type="MULTIPLY")
    mul.inputs["Factor"].default_value = 1.0
    tex = node(nt, "ShaderNodeTexImage", 100, 200, image=bpy.data.images[image], interpolation="Linear")
    uvn = node(nt, "ShaderNodeUVMap", -100, 200, uv_map=uv)
    dif = node(nt, "ShaderNodeBsdfDiffuse", -300, -100)
    dif.inputs["Color"].default_value = (1, 1, 1, 1)
    s2r = node(nt, "ShaderNodeShaderToRGB", -100, -100)
    bw = node(nt, "ShaderNodeRGBToBW", 100, -100)
    ramp = node(nt, "ShaderNodeValToRGB", 250, -100)
    cr = ramp.color_ramp
    cr.interpolation = "CONSTANT"
    while len(cr.elements) > 1:
        cr.elements.remove(cr.elements[-1])
    cr.elements[0].position = BANDS[0][0]
    cr.elements[0].color = (*BANDS[0][1], 1)
    for pos, col in BANDS[1:]:
        e = cr.elements.new(pos)
        e.color = (*col, 1)
    L = nt.links
    L.new(uvn.outputs[0], tex.inputs["Vector"])
    L.new(dif.outputs[0], s2r.inputs[0])
    L.new(s2r.outputs["Color"], bw.inputs[0])
    L.new(bw.outputs[0], ramp.inputs[0])
    if tint is None:
        L.new(tex.outputs["Color"], mul.inputs["A"])
    else:
        tn = node(nt, "ShaderNodeMix", 300, 200, data_type="RGBA", blend_type="MULTIPLY")
        tn.inputs["Factor"].default_value = 1.0
        tn.inputs["B"].default_value = (*tint, 1)
        L.new(tex.outputs["Color"], tn.inputs["A"])
        L.new(tn.outputs["Result"], mul.inputs["A"])
    L.new(ramp.outputs["Color"], mul.inputs["B"])
    L.new(mul.outputs["Result"], em.inputs["Color"])
    L.new(em.outputs[0], out.inputs[0])
    return m


def raw_material(name, image):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    out = node(nt, "ShaderNodeOutputMaterial", 600, 0)
    em = node(nt, "ShaderNodeEmission", 400, 0)
    mix = node(nt, "ShaderNodeMix", 200, 0, data_type="RGBA")
    mix.inputs["A"].default_value = (0.02, 0.6, 0.02, 1)   # green = never seen by the camera
    tex = node(nt, "ShaderNodeTexImage", -100, 0, image=bpy.data.images[image])
    uvn = node(nt, "ShaderNodeUVMap", -300, 0, uv_map="UVMap_HBM")
    nt.links.new(uvn.outputs[0], tex.inputs["Vector"])
    nt.links.new(tex.outputs["Alpha"], mix.inputs["Factor"])
    nt.links.new(tex.outputs["Color"], mix.inputs["B"])
    nt.links.new(mix.outputs["Result"], em.inputs["Color"])
    nt.links.new(em.outputs[0], out.inputs[0])
    return m


def ink_material():
    m = bpy.data.materials.get("M_Ink") or bpy.data.materials.new("M_Ink")
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    out = node(nt, "ShaderNodeOutputMaterial", 300, 0)
    em = node(nt, "ShaderNodeEmission", 100, 0)
    em.inputs["Color"].default_value = PLUM_LIN
    nt.links.new(em.outputs[0], out.inputs[0])
    m.use_backface_culling = True
    m.use_backface_culling_shadow = True
    return m


cel_material("M_Face_Cel", "T_Face_Paint")
if "T_Face_Paint_C" in bpy.data.images:
    cel_material("M_Face_Cel_C", "T_Face_Paint_C")
raw_material("M_Face_Raw", "T_Face_Proj_Raw")
if "T_Face_Proj_Raw_C" in bpy.data.images:
    raw_material("M_Face_Raw_C", "T_Face_Proj_Raw_C")
ink_material()
# eye image toned down: the concept's eye whites sit in lid shadow (linear tint)
eye = cel_material("M_Eye_Cel", "T_Bram_Eye", uv=bpy.data.objects["Bram_Eye_L"].data.uv_layers[0].name,
                   tint=(0.55, 0.45, 0.40))

head = bpy.data.objects["Bram_Head"]
hull = head.modifiers.get("Ink_Hull") or head.modifiers.new("Ink_Hull", "SOLIDIFY")
hull.thickness = 0.0015
hull.offset = 1.0
hull.use_flip_normals = True
hull.use_rim = False
hull.material_offset = 1
hull.show_render = hull.show_viewport = True
# Materials not on a mesh would be dropped on save; keep them all.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scene as S  # noqa: E402
S.clay_material()
S.eye_clay_material()
for m in bpy.data.materials:
    if m.name.startswith(("M_Face_", "M_Ink", "M_Eye_Cel", "M_Clay")):
        m.use_fake_user = True
bpy.ops.wm.save_mainfile(compress=True)
print("look built")
