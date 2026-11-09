"""Bake the (delit) concept into each target object's own UV texture.

usage: blender -b --factory-startup --disable-autoexec --python bake_concept.py -- \
        <delit.png> <outdir> <target,target,...> [density_px_per_m]

For each target object:
  1. UV unwrap (board slots: flat front projection so all slots share one layout; others: smart project)
  2. EMIT bake of the projected concept  -> what the concept shows at each texel
  3. DIFFUSE-direct bake with one light at the projector -> how squarely the concept saw that texel
     (0 = hidden or facing away: the concept never showed it)
  4. albedo = concept where seen, smooth push-pull fill where unseen; slots copy their column's best slot
Writes <outdir>/<name>.exr (linear albedo), <name>.png (sRGB preview), <name>-seen.png, bake.json.
"""
import os, sys, json, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy, bmesh
import numpy as np
import cf, layout

args = cf.argv()
delit_png, outdir, targets = args[0], args[1], args[2].split(",")
density = float(args[3]) if len(args) > 3 else 600.0
# optional guard image (concept-sized, white = trust, black = within a few px of a foreground
# silhouette): background objects treat guarded texels as unseen, because a blockout never matches the
# painted silhouette exactly and the painted edge would otherwise be baked onto the wall behind it
guard_png = args[4] if len(args) > 4 else None
# optional matte (concept-sized, white = foreground paint): foreground objects get <name>-alpha.png,
# so the parts of a blockout that lie outside the painted silhouette turn transparent
matte_png = args[5] if len(args) > 5 else None
FOREGROUND_PREFIX = ("bram_", "plant_", "chair_", "book", "sign", "mug", "rack", "case_", "desk_")
os.makedirs(outdir, exist_ok=True)

sc = cf.reset()
cf.use_gpu_cycles(sc, samples=8)
sc.render.bake.margin = 8
world = bpy.data.worlds.new("World")
world.color = (0, 0, 0)
sc.world = world
objs = layout.build(lambda n: None)
cam = cf.concept_camera()
proj = cf.projector_empty(cam)
img = bpy.data.images.load(delit_png)
guard_img = None
if guard_png:
    guard_img = bpy.data.images.load(guard_png)
    guard_img.colorspace_settings.name = "Non-Color"
matte_img = None
if matte_png:
    matte_img = bpy.data.images.load(matte_png)
    matte_img.colorspace_settings.name = "Non-Color"
alphas = {}

light = bpy.data.lights.new("Projector visibility", "POINT")
light.energy, light.shadow_soft_size = 1000, 0.0
lo = bpy.data.objects.new("Projector visibility", light)
sc.collection.objects.link(lo)
lo.location = cf.CAM_POS


def area(ob):
    me = ob.data
    s = max(ob.scale)
    return sum(p.area for p in me.polygons) * s * s


def planar_front_uv(ob):
    me = ob.data
    xs = [v.co.x for v in me.vertices]
    zs = [v.co.z for v in me.vertices]
    x0, x1, z0, z1 = min(xs), max(xs), min(zs), max(zs)
    uv = me.uv_layers.new(name="UVMap")
    for poly in me.polygons:
        back = poly.normal.y > 0.5  # faces the wall: would overwrite the front face's texels
        for li in poly.loop_indices:
            co = me.vertices[me.loops[li].vertex_index].co
            uv.data[li].uv = (0.0, 0.0) if back else ((co.x - x0) / (x1 - x0), (co.z - z0) / (z1 - z0))


def smart_uv(ob):
    bpy.ops.object.select_all(action="DESELECT")
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(66), island_margin=0.01)
    bpy.ops.object.mode_set(mode="OBJECT")


def bake_into(ob, mat, image, kind):
    ob.data.materials.clear()
    ob.data.materials.append(mat)
    nt = mat.node_tree
    tn = nt.nodes.new("ShaderNodeTexImage")
    tn.image = image
    nt.nodes.active = tn
    bpy.ops.object.select_all(action="DESELECT")
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    if kind == "EMIT":
        bpy.ops.object.bake(type="EMIT")
    else:
        bpy.ops.object.bake(type="DIFFUSE", pass_filter={"DIRECT"}, use_clear=True)


def pixels(image):
    a = np.empty(image.size[0] * image.size[1] * 4, dtype=np.float32)
    image.pixels.foreach_get(a)
    return a.reshape(image.size[1], image.size[0], 4)


def push_pull(rgb, w):
    """Fill low-weight texels from a weighted image pyramid: smooth colour continuation, no detail."""
    levels = [(rgb * w[..., None], w)]
    while levels[-1][1].shape[0] > 1 and levels[-1][1].shape[1] > 1:
        c, ww = levels[-1]
        h, wd = ww.shape[0] // 2, ww.shape[1] // 2
        c2 = c[:h * 2, :wd * 2].reshape(h, 2, wd, 2, 3).mean((1, 3))
        w2 = ww[:h * 2, :wd * 2].reshape(h, 2, wd, 2).mean((1, 3))
        levels.append((c2, w2))
    c, ww = levels[-1]
    val = c / np.maximum(ww, 1e-6)[..., None]
    def smooth_up(v, shape):
        u = np.repeat(np.repeat(v, 2, 0), 2, 1)[: shape[0], : shape[1]]
        for _ in range(2):  # 3x3 box twice ~ bilinear-ish, removes the blocky look
            p = np.pad(u, ((1, 1), (1, 1), (0, 0)), mode="edge")
            u = sum(p[i:i + shape[0], j:j + shape[1]] for i in range(3) for j in range(3)) / 9.0
        return u

    for c, ww in reversed(levels[:-1]):
        up = smooth_up(val, ww.shape)
        a = np.clip(ww * 4, 0, 1)[..., None]
        val = a * (c / np.maximum(ww, 1e-6)[..., None]) + (1 - a) * up
    return val


def save(arr_rgb, path, fmt):
    h, w = arr_rgb.shape[:2]
    im = bpy.data.images.new(os.path.basename(path), w, h, alpha=False, float_buffer=(fmt == "OPEN_EXR"))
    if fmt != "OPEN_EXR":
        im.colorspace_settings.name = "sRGB"
    rgba = np.ones((h, w, 4), dtype=np.float32)
    rgba[..., :3] = arr_rgb
    im.pixels.foreach_set(rgba.ravel())
    im.filepath_raw = path
    im.file_format = fmt
    im.save()
    bpy.data.images.remove(im)


def lin_to_srgb(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


WALL_MOUNTED = {"wall_back", "board_frame", "arches", "pillar_teal"}
report = {}
results = {}
for name in targets:
    ob = objs[name]
    if name.startswith("board_slot"):
        planar_front_uv(ob)
        size = 256
    elif name in WALL_MOUNTED:
        # seen from the front; thin sides take the edge texels, the front face gets the whole texture
        planar_front_uv(ob)
        xs = [v.co.x for v in ob.data.vertices]
        zs = [v.co.z for v in ob.data.vertices]
        side = max(max(xs) - min(xs), max(zs) - min(zs)) * density
        size = int(min(2048, max(128, 2 ** math.ceil(math.log2(side)))))
    else:
        smart_uv(ob)
        side = math.sqrt(area(ob) * 1.8) * density
        size = int(min(2048, max(128, 2 ** math.ceil(math.log2(side)))))
    # with a painted matte, the matte's guard decides what Bram hides on the wall; his oversized
    # blockout must not also shadow it in the visibility bake (it hid real wall paint around him)
    if matte_img is not None:
        bg_target = not name.startswith(FOREGROUND_PREFIX)
        for n2, o2 in objs.items():
            if n2.startswith(FOREGROUND_PREFIX) and not n2.startswith("desk_"):
                o2.visible_shadow = not bg_target
    emit_img = bpy.data.images.new(name + "_emit", size, size, alpha=False, float_buffer=True)
    vis_img = bpy.data.images.new(name + "_vis", size, size, alpha=False, float_buffer=True)
    bake_into(ob, cf.projection_emit_material("proj_" + name, proj, img), emit_img, "EMIT")
    m, nt, out = cf.new_material("vis_" + name)
    d = nt.nodes.new("ShaderNodeBsdfDiffuse")
    d.inputs["Color"].default_value = (1, 1, 1, 1)
    nt.links.new(d.outputs[0], out.inputs["Surface"])
    bake_into(ob, m, vis_img, "DIFFUSE")
    E = pixels(emit_img)[..., :3]
    V = pixels(vis_img)[..., :3].mean(-1)
    w = np.clip((V - 0.05) / (0.4 - 0.05), 0, 1)   # 0 unseen .. 1 seen squarely enough
    w = w * w * (3 - 2 * w)
    # the visibility light shines everywhere, the camera only inside its frame: outside the
    # frame the projection is clipped to black and must count as unseen
    w = w * (E.sum(-1) > 1e-4)
    if guard_img is not None and not name.startswith(FOREGROUND_PREFIX):
        g_img = bpy.data.images.new(name + "_guard", size, size, alpha=False, float_buffer=True)
        bake_into(ob, cf.projection_emit_material("guard_" + name, proj, guard_img), g_img, "EMIT")
        w = w * np.clip(pixels(g_img)[..., 0], 0, 1)
    if matte_img is not None and name.startswith(FOREGROUND_PREFIX):
        m_img = bpy.data.images.new(name + "_matte", size, size, alpha=False, float_buffer=True)
        bake_into(ob, cf.projection_emit_material("matte_" + name, proj, matte_img), m_img, "EMIT")
        mv = np.clip(pixels(m_img)[..., 0], 0, 1)
        # the matte decides wherever the concept saw the texel at all, grazing rims included (weighting by
        # w made a stand-in's rim opaque and drew a ghost of its silhouette); truly unseen texels stay opaque
        seen_any = (V > 0.01) & (E.sum(-1) > 1e-4)
        alphas[name] = np.where(seen_any, mv, 1.0)
        w = w * (mv > 0.5)                       # don't let wall paint inside the blockout seed its fill
    fill = push_pull(E, w)
    alb = w[..., None] * E + (1 - w[..., None]) * fill
    results[name] = (alb, w)
    # coverage: of the texels that land inside the concept frame, the share the concept saw
    used = (E.sum(-1) > 0) | (V > 0)
    report[name] = {"size": size, "seen_share": round(float((w > 0.5)[used].mean()) if used.any() else 0.0, 3)}

# board slots: copy the best-seen slot of the same column into slots the concept mostly hid
for name in targets:
    if name.startswith("board_slot") and report[name]["seen_share"] < 0.85:
        col = name[-1]
        donors = [n for n in targets if n.startswith("board_slot") and n.endswith(col)]
        best = max(donors, key=lambda n: report[n]["seen_share"])
        if best != name:
            alb, w = results[name]
            dalb, _ = results[best]
            results[name] = (w[..., None] * alb + (1 - w[..., None]) * dalb, w)
            report[name]["filled_from"] = best

for name, (alb, w) in results.items():
    save(alb, os.path.join(outdir, name + ".exr"), "OPEN_EXR")
    save(lin_to_srgb(alb), os.path.join(outdir, name + ".png"), "PNG")
    save(np.repeat(w[..., None], 3, -1), os.path.join(outdir, name + "-seen.png"), "PNG")
    if name in alphas:
        save(np.repeat(alphas[name][..., None], 3, -1), os.path.join(outdir, name + "-alpha.png"), "PNG")

# keep the unwrapped meshes so the animation scene reuses the exact same UVs
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(outdir, "baked.blend"))
json.dump(report, open(os.path.join(outdir, "bake.json"), "w"), indent=1)
print("BAKE_REPORT", json.dumps(report))
