# A 4-second turntable of the static bust for 3D inspection (not acting), neutral light.
#   blender -b bram_bust.blend --python scripts/turntable.py -- <out.mp4>
# The camera orbits; the bust does not move. The .blend is not saved.
import math
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lab_common  # noqa: E402

out = sys.argv[sys.argv.index("--") + 1]
sc = bpy.context.scene
lab_common.use_rig(sc, "neutral")
pivot = bpy.data.objects.new("tt_pivot", None)
sc.collection.objects.link(pivot)
pivot.location = (0.0, 0.0, -0.06)
cam = bpy.data.objects.new("tt_cam", bpy.data.cameras.new("tt_cam"))
sc.collection.objects.link(cam)
cam.parent = pivot
cam.location = (0.0, -1.15, 0.02)
cam.data.lens = 70
cam.data.clip_start = 0.01
cam.rotation_euler = (Vector((0, 0, 0)) - cam.location).to_track_quat("-Z", "Y").to_euler()
sc.frame_start, sc.frame_end = 1, 96
sc.render.fps = 24
pivot.rotation_euler = (0, 0, 0)
pivot.keyframe_insert("rotation_euler", frame=1)
pivot.rotation_euler = (0, 0, 2 * math.pi * 95 / 96)
pivot.keyframe_insert("rotation_euler", frame=96)
for fc in pivot.animation_data.action.fcurves if hasattr(pivot.animation_data.action, "fcurves") else []:
    for kp in fc.keyframe_points:
        kp.interpolation = "LINEAR"
sc.camera = cam
sc.render.resolution_x, sc.render.resolution_y = 480, 600
sc.render.resolution_percentage = 100
sc.render.use_stamp = False
sc.render.image_settings.media_type = "VIDEO" if hasattr(sc.render.image_settings, "media_type") else None
sc.render.image_settings.file_format = "FFMPEG"
sc.render.ffmpeg.format = "MPEG4"
sc.render.ffmpeg.codec = "H264"
sc.render.ffmpeg.constant_rate_factor = "HIGH"
sc.render.filepath = out
bpy.ops.render.render(animation=True)
print("turntable written", os.path.basename(out))

# Contact sheet of 8 frames, so the orbit can be checked without a video player.
import numpy as np  # noqa: E402

sheet_path = os.path.splitext(out)[0] + "_contact_sheet.png"
if hasattr(sc.render.image_settings, "media_type"):
    sc.render.image_settings.media_type = "IMAGE"
sc.render.image_settings.file_format = "PNG"
tiles = []
for f in (1, 13, 25, 37, 49, 61, 73, 85):
    sc.frame_set(f)
    p = os.path.splitext(out)[0] + f"_f{f:02d}.png"
    sc.render.filepath = p
    sc.render.resolution_x, sc.render.resolution_y = 240, 300
    bpy.ops.render.render(write_still=True)
    im = bpy.data.images.load(p, check_existing=False)
    tiles.append(np.array(im.pixels[:], dtype=np.float32).reshape(300, 240, 4))
    bpy.data.images.remove(im)
    os.remove(p)
rows = [np.concatenate(tiles[i:i + 4], axis=1) for i in (4, 0)]
canvas = np.concatenate(rows, axis=0)
img = bpy.data.images.new("sheet", canvas.shape[1], canvas.shape[0], alpha=True)
img.pixels.foreach_set(canvas.ravel())
img.filepath_raw = sheet_path
img.file_format = "PNG"
img.save()
print("contact sheet written", os.path.basename(sheet_path))
