# Shared helpers: a cylindrical UV map round the head's vertical axis, and baking
# per-vertex colours into a packed, editable image that a plain material then reads.
import math

import bpy
import numpy as np


def cylindrical_uv(me, zmin=-0.30, zmax=0.13, centre_y=0.012):
    n = len(me.vertices)
    co = np.empty(n * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    loops = np.empty(len(me.loops), dtype=np.int32)
    me.loops.foreach_get("vertex_index", loops)
    p = co[loops]
    u = (np.arctan2(p[:, 0], -(p[:, 1] - centre_y)) / (2 * math.pi)) + 0.5
    v = (p[:, 2] - zmin) / (zmax - zmin)
    totals = np.empty(len(me.polygons), dtype=np.int32)
    me.polygons.foreach_get("loop_total", totals)
    starts = np.empty(len(me.polygons), dtype=np.int32)
    me.polygons.foreach_get("loop_start", starts)
    face_of_loop = np.repeat(np.arange(len(totals)), totals)
    wrap = (np.maximum.reduceat(u, starts) - np.minimum.reduceat(u, starts)) > 0.5
    u[wrap[face_of_loop] & (u < 0.5)] = 0.999
    u = 0.5 + np.sign(u - 0.5) * (np.abs(u - 0.5) * 2) ** 0.85 / 2
    uv = me.uv_layers.get("UVMap") or me.uv_layers.new(name="UVMap")
    uv.data.foreach_set("uv", np.stack([u, v], axis=1).ravel())


def bake_colors(obj, rgb, image_name, size, material_name, roughness):
    """Bake one colour per vertex into image_name and give obj a material that reads it."""
    me = obj.data
    attr = me.color_attributes.get("Paint_Bake") or me.color_attributes.new("Paint_Bake", "FLOAT_COLOR", "POINT")
    rgba = np.concatenate([rgb, np.ones((len(rgb), 1), dtype=np.float32)], axis=1)
    attr.data.foreach_set("color", rgba.astype(np.float32).ravel())
    img = bpy.data.images.get(image_name) or bpy.data.images.new(image_name, size, size, alpha=False)
    if tuple(img.size) != (size, size):
        img.scale(size, size)
    bake_mat = bpy.data.materials.new("tmp_bake")
    bake_mat.use_nodes = True
    nt = bake_mat.node_tree
    nt.nodes.clear()
    vc = nt.nodes.new("ShaderNodeVertexColor")
    vc.layer_name = "Paint_Bake"
    emit = nt.nodes.new("ShaderNodeEmission")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.links.new(vc.outputs["Color"], emit.inputs["Color"])
    nt.links.new(emit.outputs[0], out.inputs[0])
    nt.nodes.active = tex
    me.materials.clear()
    me.materials.append(bake_mat)
    me.polygons.foreach_set("material_index", np.zeros(len(me.polygons), dtype=np.int32))
    sc = bpy.context.scene
    engine = sc.render.engine
    sc.render.engine = "CYCLES"
    sc.cycles.samples = 1
    sc.render.bake.margin = 8
    sc.render.bake.use_clear = True
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    hidden = obj.hide_get()
    obj.hide_set(False)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.bake(type="EMIT")
    sc.render.engine = engine
    obj.hide_set(hidden)
    me.color_attributes.remove(me.color_attributes["Paint_Bake"])
    bpy.data.materials.remove(bake_mat)
    img.pack()
    img.use_fake_user = True
    m = bpy.data.materials.get(material_name) or bpy.data.materials.new(material_name)
    m.use_nodes = True
    mt = m.node_tree
    b = next(nd for nd in mt.nodes if nd.type == "BSDF_PRINCIPLED")
    t = next((nd for nd in mt.nodes if nd.type == "TEX_IMAGE"), None) or mt.nodes.new("ShaderNodeTexImage")
    t.image = img
    mt.links.new(t.outputs["Color"], b.inputs["Base Color"])
    b.inputs["Roughness"].default_value = roughness
    me.materials.clear()
    me.materials.append(m)
    return img


def srgb_to_lin(c):
    c = np.asarray(c, dtype=np.float32)
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def grain(co, scale):
    """Deterministic fine variation per point, for hair grain in painted colour."""
    h = np.sin(co[:, 0] * 12.9898 * scale + co[:, 1] * 78.233 * scale + co[:, 2] * 37.719 * scale) * 43758.5453
    return h - np.floor(h)
