# Check that a board .blend opens as a normal editable file with its paint layers.
#   blender -b --disable-autoexec <file.blend> --python-exit-code 1 --python validate_blend.py [-- checkpoint]
# "checkpoint" relaxes the object count and allows paint layers that did not exist yet
# in earlier checkpoints; every image that is present must still be packed and protected.
# Any failed check raises, and --python-exit-code 1 makes Blender exit with status 1.
import sys

import bpy

CHECKPOINT = "checkpoint" in sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else False
wm = bpy.data.window_managers[0]
checks = {}
checks["window_manager_windows"] = len(wm.windows)
checks["window_scenes"] = [w.scene.name for w in wm.windows]
checks["context_scene"] = bpy.context.scene.name
checks["scenes"] = [s.name for s in bpy.data.scenes]
checks["camera"] = bpy.context.scene.camera.name if bpy.context.scene.camera else None
asset = bpy.data.collections.get("BoardCorner_Asset")
checks["asset_objects"] = len(asset.objects) if asset else 0
checks["asset_meshes_in_scene"] = sum(1 for o in bpy.context.scene.objects if o.type == "MESH")
layers = {}
for name in ("T_BoardCorner_Marks", "T_BoardCorner_BaseColor", "T_BoardCorner_Albedo"):
    image = bpy.data.images.get(name)
    layers[name] = None if image is None else (image.size[0], image.size[1], image.packed_file is not None,
                                                 image.use_fake_user, image.filepath)
checks["paint_layers"] = layers
checks["external_images"] = [i.name for i in bpy.data.images
                             if i.source == "FILE" and not i.packed_file]
checks["absolute_paths"] = [i.filepath for i in bpy.data.images
                            if i.filepath and not i.filepath.startswith("//")]
private = ("/Users/", "/home/", "/private/", "C:\\")
checks["private_in_texts"] = [t.name for t in bpy.data.texts if any(p in t.as_string() for p in private)]
checks["private_in_browsers"] = [bytes(sp.params.directory).decode(errors="replace")
                                 for sc in bpy.data.screens for a in sc.areas for sp in a.spaces
                                 if sp.type == "FILE_BROWSER" and sp.params
                                 and any(p.encode() in bytes(sp.params.directory) for p in private)]
# The file itself: a normal save holds a window-manager block (WM) and screens (SN).
# bpy.data.libraries.write leaves both out, and Blender then reports "Library file,
# loading empty scene". In background mode the session's own window hides that, so
# read the blocks from disk. Also scan every byte for private folders.
import io
import zstandard
raw = open(bpy.data.filepath, "rb").read()
data = zstandard.ZstdDecompressor().stream_reader(io.BytesIO(raw)).read() if raw[:4] == b"\x28\xb5\x2f\xfd" else raw
checks["file_wm_blocks"] = data.count(b"WM\x00\x00")
checks["file_screen_blocks"] = data.count(b"SN\x00\x00")
checks["file_private_bytes"] = [p for p in (b"/Users/", b"/home/", b"/private/", b"/tmp/") if p in data]

for key, value in checks.items():
    print(f"CHECK {key}: {value}")

assert checks["file_wm_blocks"] >= 1 and checks["file_screen_blocks"] >= 1, \
    "no window manager or screens in the file: it opens as a library file"
assert not checks["file_private_bytes"], "private folder text inside the file"

assert checks["window_manager_windows"] >= 1, "no window: Blender would treat this as a library file"
assert checks["context_scene"] == "BoardCorner", "file does not open on the BoardCorner scene"
assert all(s == "BoardCorner" for s in checks["window_scenes"]), "a window shows another scene"
assert checks["camera"] == "Cam_Reference", "scene camera is not Cam_Reference"
if CHECKPOINT:
    assert checks["asset_objects"] >= 10, "asset objects missing"
    assert any(info is not None for info in layers.values()), "no paint layer"
else:
    assert checks["asset_objects"] == 18 and checks["asset_meshes_in_scene"] == 18, "asset objects missing"
for name, info in layers.items():
    if info is None and CHECKPOINT:
        continue
    assert info is not None, f"{name} missing"
    w, h, packed, fake, path = info
    assert (w, h) == (2048, 2048) and packed and fake and path.startswith("//"), f"{name} not packed/protected"
if not CHECKPOINT:
    marks = bpy.data.images["T_BoardCorner_Marks"]
    assert sum(marks.pixels[3::4]) > 1000, "marks layer is empty"
assert not checks["external_images"] and not checks["absolute_paths"], "external or absolute image paths"
assert not checks["private_in_texts"] and not checks["private_in_browsers"], "private paths in file"
print("VALIDATION PASSED")
