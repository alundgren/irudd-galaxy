# Paint Bram's skin into an editable 2048 image on the head's UV map.
# 1. Give the head a cylindrical UV map (its cut runs up the back of the neck, under the collar).
# 2. Work out a colour for each vertex from deliberate rules: warm brown base, redder
#    nose, cheeks and ears, darker round the eyes and in the creases the geometry made,
#    mauve-brown lips, dark scalp under the curls and grey stubble shadow on the jaw.
# 3. Bake those colours into T_Bram_Skin. Paint over that image by hand in Texture Paint
#    mode afterwards; rerunning this script overwrites it.
import math

import bpy
import numpy as np

HEAD = "Bram_Head"
# Switches set by the caller's exec namespace: crease darkening from the geometry and the
# stubble and scalp paint can be left out (checkpoint 4 shows the face without them).
CREASES = globals().get("CREASES", True)
HAIR_PAINT = globals().get("HAIR_PAINT", True)
# cp7: named plum accents and warm highlights on the base-mesh head (coordinates in metres
# for the cp4+ head; eye centres match sculpt_cp4.prepare()).
ACCENTS = globals().get("ACCENTS", False)
EYES = [(-0.0323 * 1.04, -0.0725, 0.0), (0.0323 * 1.04, -0.0725, 0.0)]
EYE_R = 0.0125
SIZE = 2048


def srgb_to_lin(c):
    c = np.asarray(c, dtype=np.float32)
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def smooth_field(values, edges, n_vert, iterations):
    v = values.copy()
    deg = np.bincount(edges.ravel(), minlength=n_vert).astype(np.float32)[:, None] + 1e-6
    for _ in range(iterations):
        acc = np.zeros_like(v)
        np.add.at(acc, edges[:, 0], v[edges[:, 1]])
        np.add.at(acc, edges[:, 1], v[edges[:, 0]])
        v = 0.5 * v + 0.5 * acc / deg
    return v


def ell(p, c, r):
    q = (p - np.array(c)) / np.array(r)
    return np.sqrt(np.sum(q * q, axis=1))


def paint():
    obj = bpy.data.objects[HEAD]
    me = obj.data
    n = len(me.vertices)
    co = np.empty(n * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    no = np.empty(n * 3, dtype=np.float32)
    me.vertices.foreach_get("normal", no)
    no = no.reshape(-1, 3)
    edges = np.empty(len(me.edges) * 2, dtype=np.int32)
    me.edges.foreach_get("vertices", edges)
    edges = edges.reshape(-1, 2)
    x, y, z = co[:, 0], co[:, 1], co[:, 2]
    x = x / 1.05  # rules are written for the head before widening

    # Creases from the geometry: how far each point sits below its smoothed neighbourhood
    fine = smooth_field(co, edges, n, 6)
    broad = smooth_field(co, edges, n, 40)
    cav_f = np.sum((co - fine) * no, axis=1)
    cav_b = np.sum((co - broad) * no, axis=1)
    crease = np.clip(-cav_f / 0.00035, 0, 1)
    hollow = np.clip(-cav_b / 0.0018, 0, 1)
    ridge = np.clip(cav_b / 0.0018, 0, 1)

    base = srgb_to_lin([0.58, 0.32, 0.19]) if ACCENTS else srgb_to_lin([0.55, 0.33, 0.22])
    warm = srgb_to_lin([0.64, 0.31, 0.18]) if ACCENTS else srgb_to_lin([0.62, 0.32, 0.21])
    dark = srgb_to_lin([0.28, 0.15, 0.10])
    light = srgb_to_lin([0.68, 0.45, 0.31])
    lips = srgb_to_lin([0.40, 0.20, 0.17])
    scalp = srgb_to_lin([0.10, 0.08, 0.07])
    col = np.tile(base, (n, 1))

    def mix(mask, c, amount=1.0):
        a = np.clip(mask * amount, 0, 1)[:, None]
        col[:] = col * (1 - a) + np.asarray(c) * a

    # Redder nose, cheeks, ears
    mix(np.clip(1.4 - ell(co, (0, -0.110, -0.040), (0.022, 0.020, 0.020)), 0, 1), warm, 0.6)
    for sx in (-1, 1):
        mix(np.clip(1.3 - ell(co, (sx * 0.040, -0.080, -0.030), (0.020, 0.020, 0.018)), 0, 1), warm, 0.45)
        mix(np.clip(1.2 - ell(co, (sx * 0.078, 0.012, -0.016), (0.012, 0.022, 0.034)), 0, 1), warm, 0.5)
        # Darker skin round the eyes, strongest in the inner corners
        mix(np.clip(1.25 - ell(co, (sx * 0.030, -0.082, -0.002), (0.022, 0.012, 0.016)), 0, 1), dark, 0.55)
        # Light catches on the cheekbone and brow
        mix(np.clip(1.0 - ell(co, (sx * 0.046, -0.074, -0.018), (0.012, 0.012, 0.008)), 0, 1), light, 0.35)
        mix(np.clip(1.0 - ell(co, (sx * 0.026, -0.090, 0.024), (0.016, 0.010, 0.006)), 0, 1), light, 0.25)
        # Eyebrow skin under the hairs
        f = np.clip((sx * x - 0.010) / 0.050, 0, 1)
        bz = 0.0150 + 0.0030 * (1 - f) ** 2 + 0.0060 * np.sin(np.pi * np.minimum(1.0, f * 1.25)) - 0.006 * f ** 2
        brow = np.clip(1.0 - np.abs(z - bz) / (0.0050 * (1 - 0.4 * f)), 0, 1)
        brow *= np.clip((sx * x - 0.006) / 0.006, 0, 1) * np.clip((0.062 - sx * x) / 0.008, 0, 1) * (y < -0.06)
        mix(brow ** 0.7, srgb_to_lin([0.20, 0.12, 0.09]), 0.6)
    # Lips
    lipmask = np.maximum(np.clip(1.15 - ell(co, (0, -0.1003, -0.0702), (0.0262, 0.0115, 0.0068)), 0, 1),
                         np.clip(1.15 - ell(co, (0, -0.0988, -0.0822), (0.0238, 0.0128, 0.0090)), 0, 1))
    mix(np.clip(lipmask * 3, 0, 1) * (y < -0.098), lips, 0.75)
    # Light on the nose bridge and chin
    mix(np.clip(1.0 - ell(co, (0, -0.110, -0.020), (0.004, 0.010, 0.016)), 0, 1), light, 0.3)
    # Creases and hollows
    if CREASES:
        mix(hollow, dark, 0.55)
        mix(crease, dark * 0.6, 0.85)
        mix(ridge, light, 0.25)

    # Scalp under the curls and the stubble shadow
    if HAIR_PAINT:
        phi = np.arctan2(x, -(y - 0.010))
        a = np.abs(phi)
        low = np.interp(a, [0.0, 0.95, 1.05, 1.30, 1.42, 1.55, 1.95, 2.25, 3.2],
                        [0.06, 0.035, 0.012, -0.004, -0.030, 0.010, 0.012, -0.045, -0.062])
        hair = np.clip((z - low) / 0.006, 0, 1) * np.clip((a - 0.85) / 0.08, 0, 1)
        ear = (np.abs(np.abs(x) - 0.074) < 0.018) & (np.abs(y - 0.013) < 0.02) & (z < 0.016) & (z > -0.05)
        hair[ear] = 0
        mix(hair, scalp, 0.92)
        jtop = -0.080 - 0.45 * np.maximum(np.abs(x) - 0.024, 0.0)
        jaw = ((z < jtop) & (np.abs(x) < 0.060) & (y < -0.005) & (z > -0.128)).astype(np.float32)
        jaw *= np.clip((-0.080 - z) / 0.008, 0, 1)
        mix(smooth_field(jaw[:, None], edges, n, 30)[:, 0], srgb_to_lin([0.30, 0.21, 0.17]), 0.25)
        mustache = ((z < -0.056) & (z > -0.066) & (np.abs(x) < 0.026) & (y < -0.098)).astype(np.float32)
        mix(smooth_field(mustache[:, None], edges, n, 6)[:, 0], srgb_to_lin([0.32, 0.24, 0.20]), 0.5)

    if ACCENTS:
        X, Y, Z = co[:, 0], co[:, 1], co[:, 2]
        plum = srgb_to_lin([0.26, 0.10, 0.15])
        hi = srgb_to_lin([0.86, 0.52, 0.30])
        for ex, ey, ez in EYES:
            d = np.sqrt((X - ex) ** 2 + (Y - ey) ** 2 + (Z - ez) ** 2)
            up = (Z - ez) / (d + 1e-9)
            front = (Y < ey)
            # lash line all round the opening, heavier on the upper lid
            mix(np.exp(-((d - (EYE_R + 0.0010)) / 0.0009) ** 2) * front * (0.55 + 0.35 * (up > 0)), plum, 0.9)
            # upper-lid crease
            mix(np.exp(-((d - (EYE_R + 0.0045)) / 0.0011) ** 2) * front * np.clip((up - 0.25) / 0.2, 0, 1), plum, 0.55)
        # nostril openings and the wing creases
        for sx in (-1, 1):
            dn = np.sqrt((X - sx * 0.0095) ** 2 + ((Y + 0.1035) / 1.3) ** 2 + (Z + 0.0505) ** 2)
            mix(np.exp(-(dn / 0.0035) ** 2), plum, 0.8)
            dw = np.sqrt(((np.abs(X) - 0.0225) / 0.6) ** 2 + (Z + 0.046) ** 2) * (np.sign(X) == sx)
            mix(np.exp(-(dw / 0.0030) ** 2) * (Y < -0.085) * (np.sign(X) == sx), plum, 0.35)
            # ear hollow
            de = np.sqrt((np.abs(X) - 0.076) ** 2 + (Y - 0.010) ** 2 + (Z + 0.012) ** 2) * (np.sign(X) == sx)
            mix(np.exp(-(de / 0.008) ** 2) * (np.sign(X) == sx), plum, 0.35)
        # mouth line: the opening between the lips and the corner pockets
        mz = -0.0715 + 0.0012 * np.clip(X / 0.027, -1, 1)
        mouth = np.exp(-((Z - mz) / 0.0011) ** 2) * (np.abs(X) < 0.029) * (Y < -0.092)
        mix(mouth, plum, 0.75)
        for cx, cz in ((0.0275, -0.0700), (-0.0275, -0.0718)):
            dc = np.sqrt((X - cx) ** 2 + (Z - cz) ** 2)
            mix(np.exp(-(dc / 0.0025) ** 2) * (Y < -0.085), plum, 0.6)
        # warm highlights: nose tip, cheekbone tops, brow ridge, lower lip
        mix(np.exp(-(((X) / 0.006) ** 2 + ((Z + 0.039) / 0.006) ** 2)) * (Y < -0.108), hi, 0.45)
        for sx in (-1, 1):
            mix(np.exp(-(((X - sx * 0.050) / 0.012) ** 2 + ((Z + 0.020) / 0.007) ** 2)) * (Y < -0.055), hi, 0.30)
            mix(np.exp(-(((X - sx * 0.026) / 0.014) ** 2 + ((Z - 0.020) / 0.004) ** 2)) * (Y < -0.080), hi, 0.25)
        mix(np.exp(-((X / 0.010) ** 2 + ((Z + 0.081) / 0.003) ** 2)) * (Y < -0.098), hi, 0.25)

    attr = me.color_attributes.get("Paint_Skin") or me.color_attributes.new("Paint_Skin", "FLOAT_COLOR", "POINT")
    rgba = np.concatenate([col, np.ones((n, 1), dtype=np.float32)], axis=1)
    attr.data.foreach_set("color", rgba.ravel())

    # Cylindrical UVs round the vertical axis; the UV map is cut at the back of the neck
    loops = np.empty(len(me.loops), dtype=np.int32)
    me.loops.foreach_get("vertex_index", loops)
    p = co[loops]
    u = (np.arctan2(p[:, 0], -(p[:, 1] - 0.012)) / (2 * math.pi)) + 0.5
    zmin, zmax = -0.30, 0.13
    v = (p[:, 2] - zmin) / (zmax - zmin)
    totals = np.empty(len(me.polygons), dtype=np.int32)
    me.polygons.foreach_get("loop_total", totals)
    starts = np.empty(len(me.polygons), dtype=np.int32)
    me.polygons.foreach_get("loop_start", starts)
    face_of_loop = np.repeat(np.arange(len(totals)), totals)
    umax = np.maximum.reduceat(u, starts)
    umin = np.minimum.reduceat(u, starts)
    wrap = (umax - umin) > 0.5
    fix = wrap[face_of_loop] & (u < 0.5)
    u[fix] = 0.999
    # The face (front) gets more room: stretch u around the front half
    u = 0.5 + np.sign(u - 0.5) * (np.abs(u - 0.5) * 2) ** 0.85 / 2
    uv = me.uv_layers.get("UVMap") or me.uv_layers.new(name="UVMap")
    uv.data.foreach_set("uv", np.stack([u, v], axis=1).ravel())
    return n


def bake():
    obj = bpy.data.objects[HEAD]
    img = bpy.data.images.get("T_Bram_Skin") or bpy.data.images.new("T_Bram_Skin", SIZE, SIZE, alpha=False)
    if tuple(img.size) != (SIZE, SIZE):
        img.scale(SIZE, SIZE)
    bake_mat = bpy.data.materials.get("M_Bram_Skin_Bake") or bpy.data.materials.new("M_Bram_Skin_Bake")
    bake_mat.use_nodes = True
    nt = bake_mat.node_tree
    nt.nodes.clear()
    attr = nt.nodes.new("ShaderNodeVertexColor")
    attr.layer_name = "Paint_Skin"
    emit = nt.nodes.new("ShaderNodeEmission")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.links.new(attr.outputs["Color"], emit.inputs["Color"])
    nt.links.new(emit.outputs[0], out.inputs[0])
    nt.nodes.active = tex
    keep = list(obj.data.materials)
    obj.data.materials.clear()
    obj.data.materials.append(bake_mat)
    sc = bpy.context.scene
    old_engine = sc.render.engine
    sc.render.engine = "CYCLES"
    sc.cycles.samples = 1
    sc.render.bake.margin = 12
    sc.render.bake.use_clear = True
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    hidden = obj.hide_get()
    obj.hide_set(False)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.bake(type="EMIT")
    sc.render.engine = old_engine
    obj.data.materials.clear()
    for m in keep:
        obj.data.materials.append(m)
    obj.hide_set(hidden)
    img.pack()
    img.use_fake_user = True
    # Skin material reads only the painted image
    m = bpy.data.materials.get("M_Bram_Skin") or bpy.data.materials.new("M_Bram_Skin")
    m.use_nodes = True
    nt = m.node_tree
    b = next(nd for nd in nt.nodes if nd.type == "BSDF_PRINCIPLED")
    t = next((nd for nd in nt.nodes if nd.type == "TEX_IMAGE"), None) or nt.nodes.new("ShaderNodeTexImage")
    t.image = img
    nt.links.new(t.outputs["Color"], b.inputs["Base Color"])
    b.inputs["Roughness"].default_value = 0.62
    b.inputs["Specular IOR Level"].default_value = 0.45
    obj.data.materials.clear()
    obj.data.materials.append(m)
    # The per-vertex colours were only the bake source. Left on the mesh, a glTF export
    # would carry them as COLOR_0, which runtimes multiply into the skin texture.
    if "Paint_Skin" in obj.data.color_attributes:
        obj.data.color_attributes.remove(obj.data.color_attributes["Paint_Skin"])
    return img.name


print(paint(), "vertices painted")
print(bake(), "baked")
