# Check that the Bram bust .blend opens as a normal editable file with its painted images.
#   blender -b --disable-autoexec <file.blend> --python-exit-code 1 --python validate_blend.py
# Adapted from the board lab's check. Any failed check raises, and --python-exit-code 1
# makes Blender exit with status 1.
import io

import bpy
import zstandard

REQUIRED = ["Bram_Head", "Bram_Eye_L", "Bram_Eye_R", "Bram_Hair_Mass", "Bram_Brows",
            "Bram_Brow_Hairs", "Bram_Beard", "Bram_Moustache",
            "Bram_Cap_Crown", "Bram_Cap_Band", "Bram_Cap_Peak", "Bram_Cap_Badge", "Bram_Cap_Button",
            "Bram_Jacket", "Bram_Collar", "Bram_Shirt", "HBM_Head_Original",
            "Bram_Collar_Turned", "Bram_Lapel_L", "Bram_Lapel_R", "Bram_Shirt_Collar_L", "Bram_Shirt_Collar_R"]
IMAGES = {"T_Bram_Skin": (2048, 2048), "T_Bram_Eye": (512, 512)}

wm = bpy.data.window_managers[0]
checks = {}
checks["window_manager_windows"] = len(wm.windows)
checks["window_scenes"] = [w.scene.name for w in wm.windows]
checks["context_scene"] = bpy.context.scene.name
checks["camera"] = bpy.context.scene.camera.name if bpy.context.scene.camera else None
checks["missing_objects"] = [n for n in REQUIRED if n not in bpy.data.objects
                             or bpy.data.objects[n].type != "MESH" or not bpy.data.objects[n].data.vertices]
asset = bpy.data.collections.get("Bram_Asset")
checks["asset_meshes"] = sum(1 for o in asset.objects if o.type == "MESH") if asset else 0
checks["images"] = {n: None if n not in bpy.data.images else
                    (tuple(bpy.data.images[n].size), bpy.data.images[n].packed_file is not None,
                     bpy.data.images[n].use_fake_user, bpy.data.images[n].filepath)
                    for n in IMAGES}
skin = bpy.data.objects["Bram_Head"].data.materials[0] if "Bram_Head" in bpy.data.objects else None
checks["head_material_uses_skin"] = bool(skin and any(nd.type == "TEX_IMAGE" and nd.image and nd.image.name == "T_Bram_Skin"
                                                     for nd in skin.node_tree.nodes))
checks["head_uv"] = "UVMap" in bpy.data.objects["Bram_Head"].data.uv_layers
checks["external_images"] = [i.name for i in bpy.data.images if i.source == "FILE" and not i.packed_file]
checks["absolute_paths"] = [i.filepath for i in bpy.data.images if i.filepath and not i.filepath.startswith("//")]
private = ("/Users/", "/home/", "/private/", "C:\\")
checks["private_in_texts"] = [t.name for t in bpy.data.texts if any(p in t.as_string() for p in private)]
checks["private_in_browsers"] = [bytes(sp.params.directory).decode(errors="replace")
                                 for sc in bpy.data.screens for a in sc.areas for sp in a.spaces
                                 if sp.type == "FILE_BROWSER" and sp.params
                                 and any(p.encode() in bytes(sp.params.directory) for p in private)]
# Appended datablocks remember the file they came from; that path must not ship.
checks["weak_library_refs"] = [i.name for i in bpy.data.meshes if i.library_weak_reference is not None]
raw = open(bpy.data.filepath, "rb").read()
data = zstandard.ZstdDecompressor().stream_reader(io.BytesIO(raw)).read() if raw[:4] == b"\x28\xb5\x2f\xfd" else raw
checks["file_wm_blocks"] = data.count(b"WM\x00\x00")
checks["file_screen_blocks"] = data.count(b"SN\x00\x00")
checks["file_private_bytes"] = [p for p in (b"/Users/", b"/home/", b"/private/", b"/tmp/") if p in data]

for key, value in checks.items():
    print(f"CHECK {key}: {value}")

assert checks["file_wm_blocks"] >= 1 and checks["file_screen_blocks"] >= 1, "no window manager or screens"
assert not checks["file_private_bytes"], "private folder text inside the file"
assert not checks["weak_library_refs"], "appended meshes still record their source file"
assert checks["window_manager_windows"] >= 1, "no window"
assert checks["context_scene"] == "BramBust" and all(s == "BramBust" for s in checks["window_scenes"]), "wrong scene"
assert checks["camera"] in ("Cam_Goal", "Cam_Goal_Tilt"), "scene camera is not a goal camera"
assert not checks["missing_objects"], "asset objects missing"
for name, size in IMAGES.items():
    info = checks["images"][name]
    assert info is not None, f"{name} missing"
    assert info[0] == size and info[1] and info[2], f"{name} not packed/protected"
assert checks["head_material_uses_skin"] and checks["head_uv"], "head is not using the painted skin"
assert not checks["external_images"] and not checks["absolute_paths"], "external or absolute image paths"
assert not checks["private_in_texts"] and not checks["private_in_browsers"], "private paths in file"
print("VALIDATION PASSED")
