# Clear local folders that Blender keeps in file-browser panels, keep the paint layers
# protected, and save normally (window, workspaces and active scene stay in the file).
# Run before committing:
#   blender -b --factory-startup --disable-autoexec <file.blend> --python-exit-code 1 --python clean_for_repo.py
# --factory-startup keeps personal startup UI out when repairing an older library-style file.
# The Shading workspace has a file-browser panel; its folder field is a fixed-size buffer
# that keeps old text after the end marker, so it is overwritten with filler first.
import bpy

for screen in bpy.data.screens:
    for area in screen.areas:
        for space in area.spaces:
            if space.type != "FILE_BROWSER" or not space.params:
                continue
            p = space.params
            p.directory = b"_" * 1100
            p.filename = "_" * 250
            p.filter_search = "_" * 60
            p.directory = b"//"
            p.filename = ""
            p.filter_search = ""

for image in bpy.data.images:
    if image.name.startswith("T_BoardCorner_") and image.packed_file:
        image.use_fake_user = True

bpy.ops.wm.save_mainfile(compress=True)
print("cleaned", bpy.data.filepath.rsplit("/", 1)[-1])
