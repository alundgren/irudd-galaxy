# Step 3 (arm B). Project the concept crop onto the head's own UV map (UVMap_HBM) from
# Cam_Concept_Fit, with a visibility mask, and build two packed, editable paint layers:
#   T_Face_Proj_Raw  RGBA: concept colour as seen, alpha = how much this camera saw it
#                    (0 where hidden, back-facing, grazing or outside the concept face).
#   T_Face_Paint     RGB: the layer the cel material uses. Raw colour with the concept's
#                    broad lighting divided out, plum ink for the darkest lines, unseen
#                    areas filled from the mirrored side, then flat skin / scalp colour.
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/project_paint.py -- <repo root> [shape key]
# With a shape key name the head is evaluated with that key at 1 (arm C) and the layers
# get the suffix _C. Saves face_fit.blend.
import json
import os
import sys

import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import camfit  # noqa: E402
import landmarks as L  # noqa: E402
import pngio  # noqa: E402

LAB = os.path.dirname(HERE)
args = sys.argv[sys.argv.index("--") + 1:]
ROOT = args[0]
KEY = args[1] if len(args) > 1 else None
SUF = "_C" if KEY else ""
SIZE = 1024
UV = "UVMap_HBM"
PLUM = np.array([0x2a, 0x1a, 0x2e]) / 255.0    # direction.md 1.1 ink colour (sRGB)

# Concept face region in crop px (face, near ear, hair under the cap, beard, neck down to
# the collar). Traced on the gridded crop; it leaves out the cap, collar and background.
FACE_POLY = [(88, 250), (150, 205), (220, 185), (300, 170), (360, 165), (420, 170), (460, 195),
             (470, 240), (462, 300), (455, 340), (445, 380), (430, 420), (410, 470), (395, 505),
             (370, 540), (345, 568), (300, 572), (255, 560), (220, 532), (170, 490),
             (130, 450), (95, 420), (85, 380), (80, 320)]

head = bpy.data.objects["Bram_Head"]
# The ink shell must not take part: it would be baked and would block visibility rays.
hull = head.modifiers.get("Ink_Hull")
hull_state = (hull.show_render, hull.show_viewport) if hull else None
if hull:
    hull.show_render = hull.show_viewport = False
if head.data.shape_keys:
    for kb in head.data.shape_keys.key_blocks[1:]:
        kb.value = 1.0 if kb.name == KEY else 0.0
bpy.context.view_layer.update()
cam = bpy.data.objects["Cam_Concept_Fit"]
Rc = np.array(cam.matrix_world)[:3, :3]
Cc = np.array(cam.matrix_world)[:3, 3]
lens = cam.data.lens


def bake(kind):
    """Bake world position, normal or coverage of the evaluated head into a float image."""
    name = f"_bake_{kind}"
    img = bpy.data.images.get(name) or bpy.data.images.new(name, SIZE, SIZE, alpha=True, float_buffer=True)
    m = bpy.data.materials.new("_bake")
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    geo = nt.nodes.new("ShaderNodeNewGeometry")
    em = nt.nodes.new("ShaderNodeEmission")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    if kind == "cov":
        em.inputs["Color"].default_value = (1, 1, 1, 1)
    else:
        vm = nt.nodes.new("ShaderNodeVectorMath")
        vm.operation = "MULTIPLY_ADD"
        nt.links.new(geo.outputs["Position" if kind == "pos" else "Normal"], vm.inputs[0])
        vm.inputs[1].default_value = (0.5, 0.5, 0.5) if kind == "nrm" else (1, 1, 1)
        vm.inputs[2].default_value = (0.5, 0.5, 0.5)
        nt.links.new(vm.outputs[0], em.inputs["Color"])
    nt.links.new(em.outputs[0], out.inputs[0])
    nt.nodes.active = tex
    mats = [s.material for s in head.material_slots]
    head.data.materials.clear()
    head.data.materials.append(m)
    sc = bpy.context.scene
    eng = sc.render.engine
    sc.render.engine = "CYCLES"
    sc.cycles.samples = 1
    sc.cycles.device = "CPU"
    sc.render.bake.margin = 0
    sc.render.bake.use_clear = True
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    head.hide_render = False
    head.select_set(True)
    bpy.context.view_layer.objects.active = head
    bpy.ops.object.bake(type="EMIT", uv_layer=UV)
    sc.render.engine = eng
    head.data.materials.clear()
    for mm in mats:
        head.data.materials.append(mm)
    bpy.data.materials.remove(m)
    px = np.empty(SIZE * SIZE * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    bpy.data.images.remove(img)
    return px.reshape(SIZE, SIZE, 4)


cov = bake("cov")[:, :, 0] > 0.5
pos = bake("pos")[:, :, :3] - 0.5
nrm = bake("nrm")[:, :, :3] * 2 - 1
nrm /= np.maximum(np.linalg.norm(nrm, axis=2, keepdims=True), 1e-6)

# Occluders: the evaluated head and both eyes, in world space.
dg = bpy.context.evaluated_depsgraph_get()
verts, polys = [], []
for o in (head, bpy.data.objects["Bram_Eye_L"], bpy.data.objects["Bram_Eye_R"]):
    e = o.evaluated_get(dg)
    me = e.to_mesh()
    mw = o.matrix_world
    base = len(verts)
    verts += [tuple(mw @ v.co) for v in me.vertices]
    polys += [tuple(base + i for i in p.vertices) for p in me.polygons]
    e.to_mesh_clear()
tree = BVHTree.FromPolygons(verts, polys)

# Concept crop and its face mask (polygon, feathered by 3 px).
y0, y1, x0, x1 = L.CROP
crop = pngio.read(os.path.join(ROOT, L.REFERENCE))[y0:y1, x0:x1].astype(np.float32) / 255.0
Hc, Wc = crop.shape[:2]


def poly_mask(poly, h, w):
    yy, xx = np.mgrid[0:h, 0:w]
    inside = np.zeros((h, w), bool)
    pts = np.array(poly, float)
    j = len(pts) - 1
    for i in range(len(pts)):
        xi, yi = pts[i]
        xj, yj = pts[j]
        cond = ((yi > yy) != (yj > yy)) & (xx < (xj - xi) * (yy - yi) / (yj - yi + 1e-9) + xi)
        inside ^= cond
        j = i
    return inside.astype(np.float32)


def blur(a, s):
    """Separable gaussian blur (numpy), edge-normalised."""
    r = int(3 * s)
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / s) ** 2)
    k /= k.sum()
    pad = [(r, r), (r, r)] + [(0, 0)] * (a.ndim - 2)
    b = np.pad(a, pad, mode="edge")
    b = np.apply_along_axis(lambda v: np.convolve(v, k, mode="valid"), 0, b)
    return np.apply_along_axis(lambda v: np.convolve(v, k, mode="valid"), 1, b)


mask = np.clip(blur(poly_mask(FACE_POLY, Hc, Wc), 2.0) * 1.5 - 0.25, 0, 1)

# De-lighting estimate. Skin-like pixels (warm hue, not dark lines, beard or hair) carry
# the concept's broad light; a wide blur of them, normalised, is the light field, which is
# divided out with strength K. Lines, creases, beard and brows stay because they are much
# narrower than the blur.
lum = crop @ np.array([0.2126, 0.7152, 0.0722])
# (pink and violet rim-lit skin counts as skin, so the rim colour is divided out too)
skin_w = mask * ((crop[:, :, 0] > crop[:, :, 1] + 0.06) & (lum > 0.18)).astype(np.float32)
SIG, K = 22.0, 0.8
num = blur(crop * skin_w[:, :, None], SIG)
den = blur(skin_w, SIG)[:, :, None]
light = num / np.maximum(den, 1e-3)
ref = np.median(crop[skin_w > 0.5], axis=0)
gain = (ref[None, None, :] / np.maximum(light, 0.03)) ** K
gain = np.clip(gain, 0.4, 2.2)
clean_c = np.clip(crop * gain, 0, 1)
# Ink: the darkest marks become plum, so lines read as the game's ink, not black.
ink = np.clip((0.16 - lum) / 0.10, 0, 1)[:, :, None] * mask[:, :, None]
clean_c = clean_c * (1 - ink) + PLUM[None, None, :] * ink
# flat fill colour: the de-lit skin median, 8% darker to sit with the line-covered paint
skin_ref = np.median(clean_c[skin_w > 0.5], axis=0) * 0.92


def sample(img, xy):
    x = np.clip(xy[:, 0] - 0.5, 0, Wc - 1.001)
    y = np.clip(xy[:, 1] - 0.5, 0, Hc - 1.001)
    xi, yi = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = (x - xi)[:, None], (y - yi)[:, None]
    a = img[yi, xi] * (1 - fx) + img[yi, xi + 1] * fx
    b = img[yi + 1, xi] * (1 - fx) + img[yi + 1, xi + 1] * fx
    out = a * (1 - fy) + b * fy
    return out


def seen(P, N):
    """Projection, weight and colour lookup for world points P with normals N."""
    xy, depth = camfit.project(P, Rc, Cc, lens)
    toc = Cc[None, :] - P
    dist = np.linalg.norm(toc, axis=1)
    facing = np.einsum("ij,ij->i", N, toc / dist[:, None])
    inframe = (xy[:, 0] >= 0) & (xy[:, 0] < Wc - 1) & (xy[:, 1] >= 0) & (xy[:, 1] < Hc - 1) & (depth > 0)
    w = np.clip((facing - 0.08) / 0.30, 0, 1) * inframe
    m = np.zeros(len(P))
    m[inframe] = sample(mask[:, :, None], xy[inframe])[:, 0]
    w *= m
    vis = np.zeros(len(P), bool)
    for i in np.nonzero(w > 0)[0]:
        d = Vector(tuple(-toc[i] / dist[i]))
        hit = tree.ray_cast(Vector(tuple(Cc)), d, float(dist[i]) + 0.004)
        vis[i] = hit[0] is not None and abs(hit[3] - dist[i]) < 0.0015
    w *= vis
    return xy, w


idx = np.nonzero(cov.ravel())[0]
P = pos.reshape(-1, 3)[idx]
N = nrm.reshape(-1, 3)[idx]
xy, w = seen(P, N)
raw = np.zeros((SIZE * SIZE, 4), np.float32)
ok = w > 0
raw[idx[ok], :3] = sample(crop, xy[ok])
raw[idx, 3] = w
col = np.zeros((len(idx), 3), np.float32)
col[ok] = sample(clean_c, xy[ok])

# Mirror fill: for points the camera saw poorly, look at the mirrored point (x -> -x)
# snapped to the surface; use its colour where that one was seen.
need = np.nonzero(w < 0.95)[0]
Pm = P[need] * np.array([-1, 1, 1])
Nm = N[need] * np.array([-1, 1, 1])
Ps = np.array(Pm)
for j in range(len(need)):
    hit = tree.find_nearest(Vector(tuple(Pm[j])), 0.01)
    if hit[0] is not None:
        Ps[j] = np.array(hit[0])
xym, wm = seen(Ps, Nm)
colm = np.zeros((len(need), 3), np.float32)
okm = wm > 0
colm[okm] = sample(clean_c, xym[okm])

# Flat fill: scalp colour above the hairline rule used in the Bram lab's paint_skin.py
# (a hairline height per angle round the head; front lowered from 60 to 35 mm so it meets
# the concept's hair under the cap brim), skin colour elsewhere.
x, y, z = P[:, 0], P[:, 1], P[:, 2]
phi = np.abs(np.arctan2(x, -(y - 0.010)))
low = np.interp(phi, [0.0, 0.95, 1.05, 1.30, 1.42, 1.55, 1.95, 2.25, 3.2],
                [0.035, 0.026, 0.012, -0.004, -0.030, 0.010, 0.012, -0.045, -0.062])
hair = np.clip((z - low) / 0.006, 0, 1) * np.clip((phi - 0.85) / 0.08, 0, 1)
ear = (np.abs(np.abs(x) - 0.074) < 0.018) & (np.abs(y - 0.013) < 0.02) & (z < 0.016) & (z > -0.05)
hair[ear] = 0
hair = np.maximum(hair, np.clip((z - 0.075) / 0.01, 0, 1))      # crown, under the cap
scalp = np.array([0.13, 0.09, 0.09])
fill = skin_ref[None, :] * (1 - hair[:, None]) + scalp[None, :] * hair[:, None]

wmf = np.zeros(len(idx))
wmf[need] = wm
cm = np.zeros((len(idx), 3), np.float32)
cm[need] = colm
own = w[:, None]
mir = ((1 - w) * wmf)[:, None]
paint = col * own + cm * mir + fill * (1 - own - mir)

# Write images (sRGB byte images, packed, protected).
def store(name, rgba):
    img = bpy.data.images.get(name)
    if img is None or tuple(img.size) != (SIZE, SIZE):
        if img:
            bpy.data.images.remove(img)
        img = bpy.data.images.new(name, SIZE, SIZE, alpha=True)
    img.pixels.foreach_set(rgba.astype(np.float32).ravel())
    img.update()
    img.pack()
    img.use_fake_user = True
    return img


full = np.zeros((SIZE * SIZE, 4), np.float32)
full[:, :3] = skin_ref
full[idx, :3] = paint
full[:, 3] = 1
# grow painted texels 4 px into the uncovered gutter so mip-mapping does not pull in black
fimg = full.reshape(SIZE, SIZE, 4)
covf = cov.astype(np.float32)
for _ in range(4):
    acc = np.zeros_like(fimg[:, :, :3])
    cnt = np.zeros((SIZE, SIZE, 1))
    for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        acc += np.roll(np.roll(fimg[:, :, :3] * covf[:, :, None], dy, 0), dx, 1)
        cnt += np.roll(np.roll(covf[:, :, None], dy, 0), dx, 1)
    grow = (covf == 0) & (cnt[:, :, 0] > 0)
    fimg[grow, :3] = acc[grow] / cnt[grow]
    covf = np.maximum(covf, grow.astype(np.float32))
store("T_Face_Proj_Raw" + SUF, raw)
store("T_Face_Paint" + SUF, fimg.reshape(-1, 4))

# Diagnostics: images of the layers and the projection mask (flipped to image order).
os.makedirs(os.path.join(LAB, "renders", "work"), exist_ok=True)
flip = lambda a: a.reshape(SIZE, SIZE, -1)[::-1]
pngio.write(os.path.join(LAB, "renders", "work", f"layer_raw{SUF}.png"), flip(raw)[:, :, :3] * 255 * flip(raw)[:, :, 3:])
pngio.write(os.path.join(LAB, "renders", "work", f"layer_weight{SUF}.png"), flip(raw)[:, :, 3] * 255)
pngio.write(os.path.join(LAB, "renders", "work", f"layer_paint{SUF}.png"), flip(fimg.reshape(-1, 4))[:, :, :3] * 255)
pngio.write(os.path.join(LAB, "renders", "work", "concept_delit.png"), clean_c * 255)
pngio.write(os.path.join(LAB, "renders", "work", "concept_light_field.png"), np.clip(light / ref * 0.5, 0, 1) * 255)
stats = {"texture_size": SIZE, "uv_map": UV, "covered_texels": int(cov.sum()),
         "seen_texels_w>0.5": int((w > 0.5).sum()), "mirror_filled_w>0.5": int((((1 - w) * wmf) > 0.5).sum()),
         "skin_ref_srgb": skin_ref.round(3).tolist(), "delight_sigma_px": SIG, "delight_strength": K}
L.save(os.path.join(LAB, "data", f"projection{SUF}.json"), stats)
print(json.dumps(stats))
if hull:
    hull.show_render, hull.show_viewport = hull_state
if head.data.shape_keys:
    for kb in head.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
bpy.ops.wm.save_mainfile(compress=True)
