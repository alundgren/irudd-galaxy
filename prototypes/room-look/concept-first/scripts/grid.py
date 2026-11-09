"""Draw a labelled pixel grid over a concept image so positions can be read off it.
usage: python -I grid.py in.png out.png [step]"""
import sys
from PIL import Image, ImageDraw

src, dst = sys.argv[1], sys.argv[2]
step = int(sys.argv[3]) if len(sys.argv) > 3 else 50
im = Image.open(src).convert("RGB")
d = ImageDraw.Draw(im)
w, h = im.size
for x in range(0, w, step):
    d.line([(x, 0), (x, h)], fill=(0, 255, 255) if x % 100 == 0 else (0, 140, 140), width=1)
    if x % 100 == 0:
        d.text((x + 2, 2), str(x), fill=(255, 255, 0))
for y in range(0, h, step):
    d.line([(0, y), (w, y)], fill=(0, 255, 255) if y % 100 == 0 else (0, 140, 140), width=1)
    if y % 100 == 0:
        d.text((2, y + 2), str(y), fill=(255, 255, 0))
im.save(dst)
