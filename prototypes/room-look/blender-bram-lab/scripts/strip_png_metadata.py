# Rewrite render PNGs without Blender's metadata text (it records the .blend path).
#   blender -b --factory-startup --python strip_png_metadata.py -- <png> [<png> ...]
import sys

import bpy
import numpy as np

for path in sys.argv[sys.argv.index("--") + 1:]:
    src = bpy.data.images.load(path, check_existing=False)
    w, h = src.size
    px = np.array(src.pixels[:], dtype=np.float32)
    bpy.data.images.remove(src)
    out = bpy.data.images.new("clean", w, h, alpha=True)
    out.pixels.foreach_set(px)
    out.filepath_raw = path
    out.file_format = "PNG"
    out.save()
    bpy.data.images.remove(out)
    print("rewrote", path.rsplit("/", 1)[-1])
