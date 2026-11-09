# Check that face_fit.blend opens as a normal editable file with its paint layers.
#   blender -b --disable-autoexec face_fit.blend --python-exit-code 1 --python validate_blend.py
# Adapted from the Bram lab's check. Any failed check raises; --python-exit-code 1 makes
# Blender exit with status 1. Nothing is saved.
import io

import bpy
import zstandard

REQUIRED = ["Bram_Head", "Bram_Eye_L", "Bram_Eye_R", "HBM_Head_Original", "Cam_Concept_Fit"]
IMAGES = {"T_Face_Proj_Raw": (1024, 1024), "T_Face_Paint": (1024, 1024),
          "T_Face_Proj_Raw_C": (1024, 1024), "T_Face_Paint_C": (1024, 1024), "T_Bram_Eye": (512, 512)}
MATERIALS = ["M_Clay", "M_Clay_Eye", "M_Face_Cel", "M_Face_Cel_C", "M_Face_Raw", "M_Face_Raw_C", "M_Eye_Cel", "M_Ink"]

wm = bpy.data.window_managers[0]
c = {}
c["windows"] = len(wm.windows)
c["scene"] = bpy.context.scene.name
c["camera"] = bpy.context.scene.camera.name if bpy.context.scene.camera else None
c["missing_objects"] = [n for n in REQUIRED if n not in bpy.data.objects]
c["missing_materials"] = [n for n in MATERIALS if n not in bpy.data.materials or not bpy.data.materials[n].use_fake_user]
c["images"] = {n: None if n not in bpy.data.images else
               (tuple(bpy.data.images[n].size), bpy.data.images[n].packed_file is not None, bpy.data.images[n].use_fake_user)
               for n in IMAGES}
head = bpy.data.objects["Bram_Head"]
c["shape_keys"] = [k.name for k in head.data.shape_keys.key_blocks] if head.data.shape_keys else []
c["fit_key_value"] = head.data.shape_keys.key_blocks["Fit_Concept"].value if head.data.shape_keys else None
c["modifiers"] = [m.type for m in head.modifiers]
c["external_images"] = [i.name for i in bpy.data.images if i.source == "FILE" and not i.packed_file]
c["absolute_paths"] = [i.filepath for i in bpy.data.images if i.filepath and not i.filepath.startswith("//")]
private = ("/Users/", "/home/", "/private/", "C:\\")
c["private_in_texts"] = [t.name for t in bpy.data.texts if any(p in t.as_string() for p in private)]
c["private_in_browsers"] = [1 for sc in bpy.data.screens for a in sc.areas for sp in a.spaces
                            if sp.type == "FILE_BROWSER" and sp.params
                            and any(p.encode() in bytes(sp.params.directory) for p in private)]
c["weak_library_refs"] = [i.name for i in bpy.data.meshes if i.library_weak_reference is not None]
raw = open(bpy.data.filepath, "rb").read()
data = zstandard.ZstdDecompressor().stream_reader(io.BytesIO(raw)).read() if raw[:4] == b"\x28\xb5\x2f\xfd" else raw
c["wm_blocks"] = data.count(b"WM\x00\x00")
c["screen_blocks"] = data.count(b"SN\x00\x00")
c["private_bytes"] = [p for p in (b"/Users/", b"/home/", b"/private/", b"/tmp/", b".t3/") if p in data]
for k, v in c.items():
    print(f"CHECK {k}: {v}")
assert c["wm_blocks"] >= 1 and c["screen_blocks"] >= 1 and c["windows"] >= 1, "no window manager or screens"
assert c["scene"] == "FaceFit" and c["camera"] == "Cam_Concept_Fit", "wrong scene or camera"
assert not c["missing_objects"] and not c["missing_materials"], "objects or protected materials missing"
for n, size in IMAGES.items():
    info = c["images"][n]
    assert info is not None and info[0] == size and info[1] and info[2], f"{n} missing, not packed or not protected"
assert c["shape_keys"] == ["Basis", "Fit_Concept"] and c["fit_key_value"] == 0.0, "shape keys wrong"
assert "SUBSURF" not in c["modifiers"] and "SOLIDIFY" in c["modifiers"], "modifier stack wrong"
assert not c["external_images"] and not c["absolute_paths"], "external or absolute image paths"
assert not c["private_in_texts"] and not c["private_in_browsers"] and not c["private_bytes"], "private text in file"
assert not c["weak_library_refs"], "appended meshes still record their source file"
print("VALIDATION PASSED")
