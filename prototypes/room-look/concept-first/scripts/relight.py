"""Relight baked concept albedo with real lights; animate the lamp.

usage: blender -b <bake_dir>/baked.blend --factory-startup --disable-autoexec --python relight.py -- \
        <bake_dir> <style.json> <outdir> <job> [objects]
jobs:  still     one frame, concept camera            -> <outdir>/still.png
       tests     light-move stills                      -> <outdir>/test-*.png
       anim      72 frames from concept + 25Â° cameras   -> <outdir>/anim-<cam>/####.png

style.json (measured from the concept by the driver):
  shadow_rgb  painter's shadow colour as a multiple of the lit colour (violet: blue kept most)
  ink_srgb    outline colour
  bulb_srgb   lamp glow colour     porthole_srgb   porthole light colour
  bulb_w      lamp spot power      porthole_w      porthole power
"""
import os, sys, json, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy
from mathutils import Vector, Matrix
import cf

args = cf.argv()
bake_dir, style, outdir, job = args[0], json.load(open(args[1])), args[2], args[3]
keep = set(args[4].split(",")) if len(args) > 4 and job not in ("porthole", "room", "export") else None
os.makedirs(outdir, exist_ok=True)
sc = bpy.context.scene
cf.set_engine(sc, "EEVEE")
cf.standard_view(sc)
cf.resolution(sc, pct=50)
try:
    sc.eevee.taa_render_samples = 16
except AttributeError:
    pass

# scene from the bake file: same meshes, same UVs. Drop helpers, keep only baked objects.
for ob in list(sc.objects):
    if ob.type == "LIGHT" or ob.name.startswith("Concept projector"):
        bpy.data.objects.remove(ob)
baked = {os.path.splitext(f)[0] for f in os.listdir(bake_dir) if f.endswith(".exr")}
objs = {}
for ob in list(sc.objects):
    if ob.type != "MESH":
        continue
    if ob.name in baked and (keep is None or ob.name in keep):
        objs[ob.name] = ob
    else:
        bpy.data.objects.remove(ob)
cam = next(o for o in sc.objects if o.type == "CAMERA")

world = sc.world
world.color = tuple(0.06 * c for c in cf.lin([v * 255 for v in style["porthole_srgb"]]))

SHADOW = style["shadow_rgb"]


LIT = style.get("lit_level", 1.0)


def vmath(nt, op, a, b=None, scale=None):
    n = nt.nodes.new("ShaderNodeVectorMath")
    n.operation = op
    for i, v in enumerate((a, b)):
        if v is None:
            continue
        if isinstance(v, (tuple, list)):
            n.inputs[i].default_value = v
        else:
            nt.links.new(v, n.inputs[i])
    if scale is not None:
        if isinstance(scale, (int, float)):
            n.inputs["Scale"].default_value = scale
        else:
            nt.links.new(scale, n.inputs["Scale"])
    return n.outputs[0]


def style_material(name, exr):
    """Painter's light model, both ends measured from the concept:
         t     = light arriving here / LIT          (0 = no light, 1 = lit like the concept, up to 1.4)
         tint  = half-strength colour of that light
         final = albedo x ( SHADOW x (1 - min(t,1))  +  tint x t )
       So unlit = albedo x painter's shadow ratio (violet), lit = albedo (the delit concept colour)."""
    m, nt, out = cf.new_material("style_" + name)
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = bpy.data.images.load(exr[:-4] + ".png")  # sRGB albedo PNG (EXR round trip double-encoded)
    tex.image.colorspace_settings.name = "sRGB"
    dif = nt.nodes.new("ShaderNodeBsdfDiffuse")
    dif.inputs["Color"].default_value = (1, 1, 1, 1)
    s2r = nt.nodes.new("ShaderNodeShaderToRGB")
    nt.links.new(dif.outputs[0], s2r.inputs[0])
    S = s2r.outputs["Color"]
    bw = nt.nodes.new("ShaderNodeRGBToBW")
    nt.links.new(S, bw.inputs[0])
    lum = cf.math_node(nt, "MAXIMUM", bw.outputs[0], 1e-4)
    t = cf.math_node(nt, "MINIMUM", cf.math_node(nt, "DIVIDE", lum, LIT), 1.4)
    t1 = cf.math_node(nt, "MINIMUM", t, 1.0)
    one_minus = cf.math_node(nt, "SUBTRACT", 1.0, t1)
    hue = vmath(nt, "SCALE", S, scale=cf.math_node(nt, "DIVIDE", 1.0, lum))       # light colour, unit brightness
    tint = vmath(nt, "ADD", vmath(nt, "SCALE", hue, scale=0.5), (0.5, 0.5, 0.5))
    shadow_part = vmath(nt, "SCALE", vmath(nt, "ADD", (0, 0, 0), tuple(SHADOW)), scale=one_minus)
    lit_part = vmath(nt, "SCALE", tint, scale=t)
    mult = vmath(nt, "ADD", shadow_part, lit_part)
    final = vmath(nt, "MULTIPLY", tex.outputs["Color"], mult)
    if name in GLOWING:
        # painted glow is light, not paint: its brightest texels follow the object's "glow" value
        abw = nt.nodes.new("ShaderNodeRGBToBW")
        nt.links.new(tex.outputs["Color"], abw.inputs[0])
        mr = nt.nodes.new("ShaderNodeMapRange")
        mr.interpolation_type = "SMOOTHSTEP"
        nt.links.new(abw.outputs[0], mr.inputs["Value"])
        mr.inputs["From Min"].default_value, mr.inputs["From Max"].default_value = 0.45, 0.8
        attr = nt.nodes.new("ShaderNodeAttribute")
        attr.attribute_type = "OBJECT"
        attr.attribute_name = "glow"
        k = cf.math_node(nt, "ADD", cf.math_node(nt, "MULTIPLY", attr.outputs["Fac"], 1.1), 0.12)
        glow_col = vmath(nt, "SCALE", tex.outputs["Color"], scale=k)
        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        ca, cb = [i for i in mix.inputs if i.type == "RGBA"][:2]
        nt.links.new(mr.outputs["Result"], mix.inputs[0])
        nt.links.new(final, ca)
        nt.links.new(glow_col, cb)
        final = [o for o in mix.outputs if o.type == "RGBA"][0]
    em = nt.nodes.new("ShaderNodeEmission")
    nt.links.new(final, em.inputs["Color"])
    alpha_png = exr[:-4] + "-alpha.png"
    if os.path.exists(alpha_png):
        # matte from the bake: blockout parts outside the painted silhouette are see-through
        at = nt.nodes.new("ShaderNodeTexImage")
        at.image = bpy.data.images.load(alpha_png)
        at.image.colorspace_settings.name = "Non-Color"
        tr = nt.nodes.new("ShaderNodeBsdfTransparent")
        mx = nt.nodes.new("ShaderNodeMixShader")
        sharp = nt.nodes.new("ShaderNodeMapRange")   # matte noise left faint ghosts of the blockout
        sharp.inputs["From Min"].default_value, sharp.inputs["From Max"].default_value = 0.35, 0.65
        nt.links.new(at.outputs["Color"], sharp.inputs["Value"])
        nt.links.new(sharp.outputs["Result"], mx.inputs[0])
        try:
            m.use_transparent_shadow = True          # the see-through part must not cast a shadow
        except AttributeError:
            pass
        nt.links.new(tr.outputs[0], mx.inputs[1])
        nt.links.new(em.outputs[0], mx.inputs[2])
        nt.links.new(mx.outputs[0], out.inputs["Surface"])
        return m
    nt.links.new(em.outputs[0], out.inputs["Surface"])
    return m


GLOWING = {"lamp_shade"}
for n, ob in objs.items():
    ob.data.materials.clear()
    ob.data.materials.append(style_material(n, os.path.join(bake_dir, n + ".exr")))
    if n in GLOWING:
        ob["glow"] = 1.0

# ---- lamp rig ---------------------------------------------------------------------------
shade = objs.get("lamp_shade")
pivot = bpy.data.objects.new("Lamp pivot", None)
sc.collection.objects.link(pivot)
import layout
pivot.location = layout.LAMP_WALL_MOUNT()  # where the lamp arm meets the wall
lamp_parts = [o for n, o in objs.items() if n.startswith("lamp_")]

bulb_lin = cf.lin([v * 255 for v in style["bulb_srgb"]])
rim_z = shade.location.z - shade.dimensions.z / 2
spot = bpy.data.lights.new("Lamp spot", "SPOT")
spot.energy, spot.color = style["bulb_w"], bulb_lin
spot.spot_size, spot.spot_blend, spot.shadow_soft_size = math.radians(150), 0.5, 0.04
spot_ob = bpy.data.objects.new("Lamp spot", spot)
sc.collection.objects.link(spot_ob)
spot_ob.location = (shade.location.x, shade.location.y, rim_z - 0.02)

bulb_me = cf.uv_sphere("lamp_bulb", (shade.location.x, shade.location.y, rim_z + 0.01), (0.06, 0.06, 0.03))
bm, bnt, bout = cf.new_material("bulb glow")
bem = bnt.nodes.new("ShaderNodeEmission")
bem.inputs["Color"].default_value = (*bulb_lin, 1)
bem.inputs["Strength"].default_value = 6.0
bnt.links.new(bem.outputs[0], bout.inputs["Surface"])
bulb_me.data.materials.append(bm)
bulb_me.visible_shadow = False  # an emitter mesh must not shadow its own light (partner's lamp-pool bug)

por = bpy.data.lights.new("Porthole", "AREA")
por.energy, por.color, por.size = style["porthole_w"], cf.lin([v * 255 for v in style["porthole_srgb"]]), 0.7
por_ob = bpy.data.objects.new("Porthole", por)
sc.collection.objects.link(por_ob)
por_ob.location = (-0.9, -0.55, 1.75)
d = (Vector((0.4, -0.2, 1.5)) - por_ob.location).normalized()
por_ob.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()

# the painter's implicit key: warm, from the upper left and in front (lights the shade top, arch tops)
key = bpy.data.lights.new("Key", "SUN")
key.energy, key.color, key.angle = style.get("key_w", 2.0), cf.lin([255, 214, 170]), math.radians(10)
key.use_shadow = False   # the painter's key shades but casts nothing (its hard lamp shadow read as a ghost lamp)
key_ob = bpy.data.objects.new("Key", key)
sc.collection.objects.link(key_ob)
key_ob.rotation_euler = (Vector((0, 0, 0)) - Vector((-1.0, -1.6, 1.4))).to_track_quat("-Z", "Y").to_euler()

bpy.context.view_layer.update()
for o in lamp_parts + [spot_ob, bulb_me]:
    mw = o.matrix_world.copy()
    o.parent = pivot
    o.matrix_parent_inverse = pivot.matrix_world.inverted()
    o.matrix_world = mw

# ---- ink lines -------------------------------------------------------------------------
sc.render.use_freestyle = True
sc.render.line_thickness_mode = "ABSOLUTE"
sc.render.line_thickness = 1.2
vl = sc.view_layers[0]
fs = vl.freestyle_settings
ls = fs.linesets[0] if len(fs.linesets) else fs.linesets.new("ink")
ls.select_by_visibility = True
ls.select_silhouette, ls.select_border, ls.select_crease = True, True, True
if ls.linestyle is None:
    ls.linestyle = bpy.data.linestyles.new("ink")
ls.linestyle.color =cf.lin([v * 255 for v in style["ink_srgb"]])
ls.linestyle.thickness = 1.4
# ink only where geometry matches the paint (architecture, board, lamp, desk). Foreground blockouts
# (Bram, props, plant, chair) are rough stand-ins: their outlines would draw the wrong shape over the
# painted ink. The space layers join the same excluded collection in setup_space.
NOINK = bpy.data.collections.new("noink")
sc.collection.children.link(NOINK)
NOINK.hide_render = False
import piece1_targets as _T
for n, o in objs.items():
    if n.startswith(_T.FOREGROUND) and not n.startswith("desk_"):
        NOINK.objects.link(o)
        if os.path.exists(os.path.join(bake_dir, n + "-alpha.png")):
            # an oversized stand-in would shadow the wall with its own (wrong) silhouette; the
            # painted contact shading is already in the bake
            o.visible_shadow = False
ls.select_by_collection = True
ls.collection = NOINK
ls.collection_negation = "EXCLUSIVE"

ref_cam = cam
side_cam = cf.orbit_camera("yaw+25", 25, target=Vector((0.35, -0.3, 1.9)))


def set_sway(rz_deg, rx_deg=0.0):
    pivot.rotation_euler = (math.radians(rx_deg), 0, math.radians(rz_deg))


def set_light(k):
    spot.energy = style["bulb_w"] * k
    bem.inputs["Strength"].default_value = 6.0 * k
    shade["glow"] = k


if job == "views":
    # still lamp, several cameras: concept, 25° left/right, 50° right, higher
    cams = [("concept", ref_cam), ("yaw-25", cf.orbit_camera("v-25", -25, target=Vector((0.35, -0.3, 1.9)))),
            ("yaw+25", side_cam), ("yaw+50", cf.orbit_camera("v+50", 50, target=Vector((0.35, -0.3, 1.9)))),
            ("high", cf.orbit_camera("vhigh", 0, 25, target=Vector((0.35, -0.3, 1.6))))]
    for label, c in cams:
        cf.render(sc, c, os.path.join(outdir, f"view-{label}.png"))
elif job == "albedo":
    # diagnostic: the baked albedo alone, no light model, no lines
    sc.render.use_freestyle = False
    for n, ob in objs.items():
        m, nt, out = cf.new_material("albedo_" + n)
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = bpy.data.images.load(os.path.join(bake_dir, n + ".png"))
        tex.image.colorspace_settings.name = "sRGB"
        em = nt.nodes.new("ShaderNodeEmission")
        nt.links.new(tex.outputs["Color"], em.inputs["Color"])
        nt.links.new(em.outputs[0], out.inputs["Surface"])
        ob.data.materials.clear()
        ob.data.materials.append(m)
    cf.resolution(sc, pct=100)
    cf.render(sc, ref_cam, os.path.join(outdir, "albedo.png"))
elif job == "still":
    cf.render(sc, ref_cam, os.path.join(outdir, "still.png"))
elif job == "tests":
    tests = [("rest", 0, 1, 1), ("swung", 16, 1, 1), ("lamp-off", 0, 0, 1), ("porthole-off", 0, 1, 0)]
    for name, rz, kl, kp in tests:
        set_sway(rz)
        set_light(kl)
        por.energy = style["porthole_w"] * kp
        cf.render(sc, ref_cam, os.path.join(outdir, f"test-{name}.png"))
    set_sway(16)
    set_light(1)
    por.energy = style["porthole_w"]
    cf.render(sc, side_cam, os.path.join(outdir, "test-swung-side.png"))
def animate_lamp(N, dips):
    import random
    random.seed(7)
    sc.frame_start, sc.frame_end = 1, N
    period = 72
    for f in range(1, N + 1):
        t = (f - 1) / period
        set_sway(3.5 * math.sin(2 * math.pi * t), 1.2 * math.sin(4 * math.pi * t + 0.6))
        pivot.keyframe_insert("rotation_euler", frame=f)
        k = dips.get(f, 1.0) * (1 + 0.05 * (random.random() - 0.5))
        set_light(k)
        spot.keyframe_insert("energy", frame=f)
        bem.inputs["Strength"].keyframe_insert("default_value", frame=f)
        shade.keyframe_insert('["glow"]', frame=f)


def render_anim(cams):
    for label, c in cams:
        sc.camera = c
        os.makedirs(os.path.join(outdir, "anim-" + label), exist_ok=True)
        sc.render.filepath = os.path.join(outdir, "anim-" + label, "")
        sc.render.image_settings.file_format = "PNG"
        bpy.ops.render.render(animation=True)


def space_layer(name, png, px_rect, dist, extend=False, blink=False, img_rect=None):
    """Plane parallel to the concept image plane, `dist` m in front of the camera, textured with
    `png` so that from the concept camera it lands exactly on the concept pixels in px_rect."""
    x0, y0, x1, y1 = px_rect
    Y = cf.CAM_POS.y + dist
    corners = [(x0, y1), (x1, y1), (x1, y0), (x0, y0)]
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(cf.px_to_world_on_plane(x, y, Y)) for x, y in corners], [], [[0, 3, 2, 1]])
    uv = me.uv_layers.new(name="UVMap")
    for li in me.polygons[0].loop_indices:
        x, y = corners[me.loops[li].vertex_index]
        ix, iy, iw, ih = img_rect or (0, 0, cf.CONCEPT_W, cf.CONCEPT_H)  # image covers these concept pixels
        uv.data[li].uv = ((x - ix) / iw, 1 - (y - iy) / ih)
    m, nt, out = cf.new_material("space_" + name)
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = bpy.data.images.load(png)
    tex.extension = "EXTEND" if extend else "CLIP"
    col = tex.outputs["Color"]
    if blink:
        # warm window lights blink: per-pixel phase from noise, time from the object's "t" property
        sep = nt.nodes.new("ShaderNodeSeparateColor")
        nt.links.new(col, sep.inputs[0])
        warm = cf.math_node(nt, "SUBTRACT", sep.outputs[0], sep.outputs[2])
        wmask = nt.nodes.new("ShaderNodeMapRange")
        nt.links.new(warm, wmask.inputs["Value"])
        wmask.inputs["From Min"].default_value, wmask.inputs["From Max"].default_value = 0.15, 0.45
        noise = nt.nodes.new("ShaderNodeTexNoise")
        noise.inputs["Scale"].default_value = 40.0
        attr = nt.nodes.new("ShaderNodeAttribute")
        attr.attribute_type, attr.attribute_name = "OBJECT", "t"
        phase = cf.math_node(nt, "ADD", cf.math_node(nt, "MULTIPLY", attr.outputs["Fac"], 4.4), cf.math_node(nt, "MULTIPLY", noise.outputs["Fac"], 25.0))
        wave = cf.math_node(nt, "MULTIPLY", cf.math_node(nt, "SINE", phase, 0.0), 0.7)
        gain = cf.math_node(nt, "ADD", cf.math_node(nt, "MULTIPLY", wmask.outputs["Result"], wave), 1.0)
        col = vmath(nt, "SCALE", col, scale=gain)
    em = nt.nodes.new("ShaderNodeEmission")
    nt.links.new(col, em.inputs["Color"])
    tr = nt.nodes.new("ShaderNodeBsdfTransparent")
    mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(tex.outputs["Alpha"], mix.inputs[0])
    nt.links.new(tr.outputs[0], mix.inputs[1])
    nt.links.new(em.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], out.inputs["Surface"])
    me.materials.append(m)
    ob = bpy.data.objects.new(name, me)
    SPACE.objects.link(ob)          # own collection: no ink lines on the layer planes
    ob.visible_shadow = False
    return ob


def setup_space(layers, N):
    """Porthole view as parallax layers (sky 90 m, tower 25 m with blinking windows, platform 12 m drifting)."""
    global SPACE
    SPACE = bpy.data.collections.new("space")
    sc.collection.children.link(SPACE)
    pad = json.load(open(os.path.join(layers, "layers.json")))["pad"]
    space_layer("space_sky", os.path.join(layers, "sky-plate.png"), (-3000, -3000, 4000, 4000), 90.0, extend=True,
                img_rect=(-pad, -pad, cf.CONCEPT_W + 2 * pad, cf.CONCEPT_H + 2 * pad))
    tower = space_layer("space_tower", os.path.join(layers, "layer-tower.png"), (100, 170, 260, 480), 25.0, blink=True)
    plat = space_layer("space_platform", os.path.join(layers, "layer-left.png"), (0, 290, 160, 430), 12.0)
    tower["t"] = 0.0
    for f in (1, N):
        tower["t"] = (f - 1) / 24.0
        tower.keyframe_insert('["t"]', frame=f)
        plat.location = (-0.9 * (f - 1) / (N - 1), 0, 0)
        plat.keyframe_insert("location", frame=f)
    for fc in (tower.animation_data.action.fcurves if hasattr(tower.animation_data.action, "fcurves") else []):
        for kp in fc.keyframe_points:
            kp.interpolation = "LINEAR"
    for o in SPACE.objects:
        NOINK.objects.link(o)


def setup_board(N, base, flip_len=4):
    """Split-flap arrivals: every slot flutters through 3 cards (the next rows' real concept content),
    cascading top row first."""
    import splitflap
    card_mats = {}

    def mat_for_card(key):
        if key not in card_mats:
            card_mats[key] = style_material("card_" + key, os.path.join(bake_dir, f"board_slot_{key}.exr"))
        return card_mats[key]

    for i in range(6):
        for j in range(3):
            slot = objs[f"board_slot_{i}{j}"]
            cards = [f"{(i + k) % 6}{j}" for k in range(4)]
            starts = [base + i * 4 + j * 2 + k * 5 for k in range(3)]
            splitflap.build(slot, cards, mat_for_card, starts, flip_len, N)


def setup_breathing(N, period=84):
    """Bram breathes: everything named bram_* hangs from a hip pivot that rises and swells slightly."""
    parts = [o for n, o in objs.items() if n.startswith("bram_")]
    if not parts:
        return
    torso = objs["bram_torso"]
    hip = bpy.data.objects.new("Bram hip", None)
    sc.collection.objects.link(hip)
    hip.location = (torso.location.x, torso.location.y, torso.location.z - torso.dimensions.z / 2)
    bpy.context.view_layer.update()
    for o in parts:
        mw = o.matrix_world.copy()
        o.parent = hip
        o.matrix_parent_inverse = hip.matrix_world.inverted()
        o.matrix_world = mw
    for f in range(1, N + 1):
        b = math.sin(2 * math.pi * (f - 1) / period)
        hip.scale = (1 + 0.006 * b, 1 + 0.006 * b, 1 + 0.010 * b)
        hip.rotation_euler = (math.radians(0.6 * math.sin(2 * math.pi * (f - 1) / (period * 1.7))), 0, 0)
        hip.keyframe_insert("scale", frame=f)
        hip.keyframe_insert("rotation_euler", frame=f)


if job == "porthole":
    N = 96
    animate_lamp(N, {40: 0.45, 41: 0.85, 42: 0.55})
    setup_space(args[4], N)
    # camera 1: the concept framing, dollying 0.8 m to the right and back (orientation and shift kept)
    dolly = cam.copy()
    dolly.data = cam.data.copy()
    sc.collection.objects.link(dolly)
    for f in range(1, N + 1):
        s = math.sin(math.pi * (f - 1) / (N - 1))
        dolly.location = cf.CAM_POS + Vector((0.8 * s, 0, 0))
        dolly.keyframe_insert("location", frame=f)
    # camera 2: walked up to the window along the concept's own line of sight through it, swaying side to side
    import layout
    pc = layout.porthole_center()
    inward = Vector((math.sin(layout.LEFT_ANGLE), -math.cos(layout.LEFT_ANGLE), 0))
    wcam_data = bpy.data.cameras.new("window")
    wcam_data.lens = 35
    wcam_data.clip_end = 200
    wcam = bpy.data.objects.new("window", wcam_data)
    sc.collection.objects.link(wcam)
    side = Vector((math.cos(layout.LEFT_ANGLE), math.sin(layout.LEFT_ANGLE), 0))
    sight = (pc - cf.CAM_POS).normalized()
    across = sight.cross(Vector((0, 0, 1))).normalized()
    for f in range(1, N + 1):
        s = math.sin(2 * math.pi * (f - 1) / (N - 1))
        wcam.location = pc - sight * 1.0 + across * (0.25 * s)
        d = (pc + sight * 20.0 - wcam.location).normalized()
        wcam.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
        wcam.keyframe_insert("location", frame=f)
        wcam.keyframe_insert("rotation_euler", frame=f)
    render_anim((("dolly", dolly), ("window", wcam)))
elif job == "anim":
    animate_lamp(72, {30: 0.35, 31: 0.8, 32: 0.45, 33: 0.9, 55: 0.6, 56: 0.95})
    render_anim((("concept", ref_cam), ("side", side_cam)))
elif job == "board":
    N = 96
    animate_lamp(N, {70: 0.4, 71: 0.85, 72: 0.5, 73: 0.9})
    setup_board(N, 18)
    import layout
    bc = layout.px_on_y(722, 440, -0.08)
    board_cam = cf.orbit_camera("board", 24, 6, target=bc, dist=1.25, lens=40)
    render_anim((("concept", ref_cam), ("board", board_cam)))
elif job == "export":
    # self-contained board + porthole for appending into another scene (concept-camera world,
    # back wall at Y=0, Z up, metres). Textures packed. EEVEE light model (Shader to RGB) kept.
    N = 144
    setup_board(N, 30)
    setup_space(args[4], N)
    sc.frame_start, sc.frame_end = 1, N
    board_c = bpy.data.collections.new("board")
    port_c = bpy.data.collections.new("porthole")
    sc.collection.children.link(board_c)
    sc.collection.children.link(port_c)
    SPACE.name = "porthole_space"
    sc.collection.children.unlink(SPACE)
    port_c.children.link(SPACE)
    for ob in list(sc.collection.objects):
        n = ob.name
        dest = board_c if n.startswith(("board_frame", "board_slot")) else \
            port_c if n in ("porthole_tube", "porthole_bezel", "wall_left") else None
        if dest is not None:
            sc.collection.objects.unlink(ob)
            dest.objects.link(ob)
        elif ob.type != "CAMERA":
            bpy.data.objects.remove(ob)
    port_c.objects.unlink(objs["wall_left"])   # the partner's room has its own wall
    bpy.data.objects.remove(objs["wall_left"])
    # roots: board fitted into the partner's recess (inner opening W x H centred at x,z, front face y);
    # porthole root at the porthole centre, so it can be moved and turned as one piece
    bpy.context.view_layer.update()
    W, H, CX, CZ, FY = [float(v) for v in (args[5] if len(args) > 5 else "1.06,0.74,0.89,1.33,0.755").split(",")]
    fr = objs["board_frame"]
    bb = [fr.matrix_world @ Vector(c) for c in fr.bound_box]
    lo = Vector([min(v[i] for v in bb) for i in range(3)])
    hi = Vector([max(v[i] for v in bb) for i in range(3)])
    s = min(W / (hi.x - lo.x), H / (hi.z - lo.z))
    import layout

    def root_for(coll, name, origin):
        r = bpy.data.objects.new(name, None)
        coll.objects.link(r)
        r.location = origin
        bpy.context.view_layer.update()
        for o in list(coll.all_objects):
            if o is not r and o.parent is None and o.name != "porthole_view":
                mw = o.matrix_world.copy()
                o.parent = r
                o.matrix_parent_inverse = r.matrix_world.inverted()
                o.matrix_world = mw
        return r

    broot = root_for(board_c, "board_root", Vector(((lo.x + hi.x) / 2, lo.y, (lo.z + hi.z) / 2)))
    broot.location = (CX, FY, CZ)
    broot.scale = (s, s, s)
    # space layers hang from their own "porthole_view" empty at the opening, its -Z along the concept's
    # sight line through it: a room aims that empty along ITS camera's sight line, independent of the tube
    pc = layout.porthole_center()
    sight = (pc - cf.CAM_POS).normalized()
    view = bpy.data.objects.new("porthole_view", None)
    view.empty_display_type = "SINGLE_ARROW"
    SPACE.objects.link(view)
    view.location = pc
    view.rotation_euler = sight.to_track_quat("-Z", "Y").to_euler()
    bpy.context.view_layer.update()
    for o in list(SPACE.objects):
        if o is not view and o.parent is None:
            mw = o.matrix_world.copy()
            o.parent = view
            o.matrix_parent_inverse = view.matrix_world.inverted()
            o.matrix_world = mw
    # shallower tube (the concept's 0.35 m reads as a grey tyre from oblique room cameras): scale it
    # along the wall normal about the wall face
    depth_k = float(args[6]) if len(args) > 6 else 0.35
    normal = Vector((math.sin(layout.LEFT_ANGLE), -math.cos(layout.LEFT_ANGLE), 0)).normalized()
    S = Matrix.Identity(4)
    for i in range(3):
        for j in range(3):
            S[i][j] += (depth_k - 1) * normal[i] * normal[j]
    tube = objs["porthole_tube"]
    tube.matrix_world = Matrix.Translation(pc) @ S @ Matrix.Translation(-pc) @ tube.matrix_world
    root_for(port_c, "porthole_root", pc)
    print("EXPORT board scale", round(s, 3), "frame m", [round(v, 3) for v in (hi - lo)])
    bpy.ops.outliner.orphans_purge(do_recursive=True)
    bpy.ops.file.pack_all()
elif job == "room":
    # piece 5: the whole room at once. Lamp sways and flickers, board flips, porthole parallax and
    # blinking tower, Bram breathes; one camera pushes in as if leaning toward the desk, one stands aside.
    N = 144
    animate_lamp(N, {50: 0.4, 51: 0.85, 52: 0.5, 53: 0.9, 110: 0.6, 111: 0.95})
    setup_space(args[4], N)
    setup_board(N, 30)
    setup_breathing(N)
    push = cam.copy()
    push.data = cam.data.copy()
    sc.collection.objects.link(push)
    for f in range(1, N + 1):
        u = (f - 1) / (N - 1)
        e = u * u * (3 - 2 * u)
        push.location = cf.CAM_POS + Vector((0.30 * e, 0.55 * e, -0.04 * e))
        push.keyframe_insert("location", frame=f)
    render_anim((("push", push), ("side", side_cam)))
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(outdir, "scene-" + job + ".blend"))

