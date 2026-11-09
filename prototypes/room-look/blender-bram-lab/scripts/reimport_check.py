# Render the authored bust and its GLB export under identical conditions.
#   blender -b bram_bust.blend --python-exit-code 1 --python scripts/reimport_check.py -- <glb> <out_dir>
# Source and reimport live in separate scenes: the check scene is a copy of BramBust
# (same cameras, lights, worlds, render and colour settings) with Bram_Asset unlinked
# and the GLB imported in its place. The .blend is never saved.
import json
import os
import struct
import sys

import bpy
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lab_common  # noqa: E402

glb, out_dir = sys.argv[sys.argv.index("--") + 1:][:2]
src = bpy.data.scenes["BramBust"]
chk = src.copy()
chk.name = "GLB_Reimport_Check"
chk.collection.children.unlink(bpy.data.collections["Bram_Asset"])
for c in list(chk.collection.children):
    if c.name == "HBM_Original":
        chk.collection.children.unlink(c)
with bpy.context.temp_override(scene=chk, view_layer=chk.view_layers[0]):
    bpy.ops.import_scene.gltf(filepath=glb)
print("reimported meshes:", sum(1 for o in chk.objects if o.type == "MESH"))

views = (("Cam_Goal_Tilt", "neutral", 570, 900), ("Cam_Goal_Tilt", "goallit", 570, 900),
         ("Cam_ThreeQuarter", "neutral", 800, 800), ("Cam_Side", "neutral", 800, 800))
paths = {}
for label, scene in (("src", src), ("glb", chk)):
    bpy.context.window.scene = scene if bpy.context.window else None
    for cam, light, w, h in views:
        lab_common.use_rig(scene, light)
        p = os.path.join(out_dir, f"final_export_{label}_{cam[4:].lower()}_{light}.png")
        lab_common.render(scene, cam, p, w, h)
        paths[(label, cam, light)] = p


def load(p):
    im = bpy.data.images.load(p, check_existing=False)
    a = np.array(im.pixels[:], dtype=np.float32).reshape(im.size[1], im.size[0], 4)
    bpy.data.images.remove(im)
    return a


rows = []
for cam, light, w, h in views:
    a = load(paths[("src", cam, light)])
    b = load(paths[("glb", cam, light)])
    d = np.abs(a[..., :3] - b[..., :3]) * 255
    print(f"DIFF {cam} {light}: mean {d.mean():.2f}/255, max {d.max():.0f}, "
          f"pixels > 24: {100 * (d.max(axis=2) > 24).mean():.1f}%")
    diff = np.concatenate([np.clip(d / 64, 0, 1), np.ones((h, w, 1), np.float32)], axis=2)
    gap = np.ones((h, 8, 4), np.float32)
    rows.append((h, np.concatenate([a, gap, b, gap, diff], axis=1)))
width = max(r.shape[1] for _, r in rows)
canvas = []
for h, r in rows:
    pad = np.ones((h, width, 4), np.float32)
    pad[:, :r.shape[1]] = r
    canvas.append(pad)
    canvas.append(np.ones((8, width, 4), np.float32))
canvas = np.concatenate(canvas[::-1][1:], axis=0)
img = bpy.data.images.new("cmp", canvas.shape[1], canvas.shape[0], alpha=True)
img.pixels.foreach_set(canvas.ravel())
img.filepath_raw = os.path.join(out_dir, "final_export_reimport_comparison.png")
img.file_format = "PNG"
img.save()

# What the GLB actually holds: read its JSON chunk
raw = open(glb, "rb").read()
n = struct.unpack("<I", raw[12:16])[0]
gj = json.loads(raw[20:20 + n])
print("GLB extensionsUsed:", gj.get("extensionsUsed"))
print("GLB meshes:", len(gj.get("meshes", [])), "materials:", len(gj.get("materials", [])),
      "images:", [(i.get("name"), i.get("mimeType")) for i in gj.get("images", [])])
for m in gj.get("materials", []):
    pbr = m.get("pbrMetallicRoughness", {})
    print("GLB material", m.get("name"), "baseColorTexture" in pbr, pbr.get("baseColorFactor"),
          "doubleSided" if m.get("doubleSided") else "")
cols = sum(1 for me in gj.get("meshes", []) for p in me["primitives"] if "COLOR_0" in p["attributes"])
print("GLB primitives with COLOR_0:", cols)
