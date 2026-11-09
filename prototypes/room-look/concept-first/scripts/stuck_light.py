"""How much painted light is stuck in a texture: with the lamp OFF, brightness of the wall/board near
the lamp divided by the same surfaces far from it. Real light is off, so anything above the concept's
own material variation is painted light that will not move.
usage: python -I stuck_light.py id.png id.json render_lamp_off.png [more renders...]"""
import json, sys
import numpy as np
from PIL import Image

ids = np.asarray(Image.open(sys.argv[1]).convert("RGB"))[:, :, 0].astype(int)
names = {n: int(k) for k, n in json.load(open(sys.argv[2])).items()}
surf = np.isin(ids, [names["wall_back"], names["board_frame"]] + [v for n, v in names.items() if n.startswith("board_slot")])
yy, xx = np.mgrid[0:ids.shape[0], 0:ids.shape[1]]
r2 = (xx - 579) ** 2 + (yy - 200) ** 2
near, far = surf & (r2 < 160 ** 2), surf & (r2 > 320 ** 2) & (yy < 640)
for p in sys.argv[3:]:
    im = Image.open(p).convert("RGB").resize((ids.shape[1], ids.shape[0]), Image.BILINEAR)
    a = np.asarray(im).astype(float) / 255
    L = a @ [0.2126, 0.7152, 0.0722]
    print(f"{p[-60:]:60s} near/far brightness {L[near].mean() / L[far].mean():.3f}")
