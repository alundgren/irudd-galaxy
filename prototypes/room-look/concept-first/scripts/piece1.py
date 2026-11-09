"""Piece 1 driver: delight -> measure painter's light -> bake -> relight -> animate.
usage: python -I scripts/piece1.py [steps]   steps: any of delight,bake,still,tests,anim (default all)"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from PIL import Image
import tools, delight

S = tools.STUDY
out = os.path.join(S, "pieces", "01-lamp-corner")
p0 = os.path.join(S, "pieces", "00-layout")
concept = os.path.join(S, "concept", "room.png")
steps = sys.argv[1].split(",") if len(sys.argv) > 1 else ["delight", "bake", "still", "tests", "anim"]
from piece1_targets import TARGETS, FOREGROUND, ROOM
LIGHT_SURFACES = ["wall_back", "arches", "board_frame", "desk_top", "lamp_shade"]

if "delight" in steps:
    # the concept's caption box "E / Ink, depth, old technology" is not part of the room
    ANNOTATIONS = [(0, 0, 442, 46)]
    # after matte.py: delight with the painted silhouettes, not the stand-in blockouts
    painted = os.path.join(out, "id-painted.png")
    idp = painted if os.path.exists(painted) else os.path.join(p0, "id.png")
    rep = delight.delight(concept, idp, os.path.join(p0, "id.json"),
                          os.path.join(out, "concept-delit.png"), annotations=ANNOTATIONS)
    json.dump(rep, open(os.path.join(out, "painter-light.json"), "w"), indent=1)
    ratios = np.array([rep["objects"][n]["shadow_ratio_rgb"] for n in LIGHT_SURFACES])
    shadow = ratios.mean(0)
    con = np.asarray(Image.open(concept).convert("RGB")).astype(float) / 255
    L = con @ [0.2126, 0.7152, 0.0722]
    ink = np.median(con[(L > np.percentile(L, 1)) & (L <= np.percentile(L, 5))], axis=0)
    ids = np.asarray(Image.open(os.path.join(p0, "id.png")).convert("RGB"))[:, :, 0].astype(int)
    names = {n: int(k) for k, n in json.load(open(os.path.join(p0, "id.json"))).items()}
    shade = ids == names["lamp_shade"]
    # lamp colour = how the same material (back wall) changes colour near the bulb (crop 579,200)
    # versus far from it; normalised so the strongest channel is 1. (concept/delit can't show this:
    # delighting keeps hue.)
    delit_img = np.asarray(Image.open(os.path.join(out, "concept-delit.png")).convert("RGB")).astype(float) / 255
    yy, xx = np.mgrid[0:con.shape[0], 0:con.shape[1]]
    r2 = (xx - 579) ** 2 + (yy - 200) ** 2
    wall = ids == names["wall_back"]
    lin = lambda a: np.where(a <= 0.04045, a / 12.92, ((a + 0.055) / 1.055) ** 2.4)
    ratio = lin(con[wall & (r2 < 160 ** 2)]).mean(0) / lin(con[wall & (r2 > 320 ** 2)]).mean(0)
    bulb = ratio / ratio.max()
    bulb = np.where(bulb <= 0.0031308, bulb * 12.92, 1.055 * bulb ** (1 / 2.4) - 0.055)  # back to sRGB
    sat = lambda a: float((a.max(-1) - a.min(-1)).mean())
    tm = np.isin(ids, [names[n] for n in TARGETS if n in names])
    print("saturation concept vs delit over targets:", round(sat(con[tm]), 3), round(sat(delit_img[tm]), 3))
    # porthole light = median concept colour inside the window opening (ellipse from porthole_layers.py)
    win = ((xx - 127) / 116) ** 2 + ((yy - 300) / 163) ** 2 < 1
    porthole = np.median(con[win], axis=0)
    style = {"shadow_rgb": [float(v) for v in shadow], "ink_srgb": [float(v) for v in ink],
             "bulb_srgb": [float(v) for v in bulb], "porthole_srgb": [float(v) for v in porthole],
             "bulb_w": 15.0, "porthole_w": 25.0,
             "_how": "shadow_rgb = mean painter shadow/lit ratio over " + ",".join(LIGHT_SURFACES) +
                     "; ink = median colour of the 1-5% darkest concept pixels; bulb = back-wall colour within 160 px of the bulb / beyond 320 px (linear, max channel 1); bulb_w, porthole_w, key_w, lit_level hand-tuned against compare.py"}
    old = os.path.join(out, "style.json")
    if os.path.exists(old):  # keep hand-tuned light powers across reruns
        o = json.load(open(old))
        for k in ("bulb_w", "porthole_w", "key_w", "lit_level"):
            if k in o:
                style[k] = o[k]
    json.dump(style, open(old, "w"), indent=1)
    print("style", style)
    # guard band: 12 px around foreground silhouettes, where blockout and painted outline disagree
    from PIL import ImageFilter
    fg = np.isin(ids, [k for n, k in names.items() if n.startswith(FOREGROUND)])
    fgd = np.asarray(Image.fromarray((fg * 255).astype(np.uint8)).filter(ImageFilter.MaxFilter(25))) > 127
    Image.fromarray(((~fgd) * 255).astype(np.uint8)).save(os.path.join(out, "guard.png"))

if "bake" in steps:
    # first pass: guard from the blockout. After scripts/matte.py has run, the painted matte gives a
    # tighter guard and the foreground alphas (bake again).
    matte = os.path.join(out, "matte.png")
    extra = [os.path.join(out, "guard-matte.png"), matte] if os.path.exists(matte) else [os.path.join(out, "guard.png")]
    for src, d in ((os.path.join(out, "concept-delit.png"), "bake"), (concept, "bake-raw")):
        tools.blender("bake_concept.py", src, os.path.join(out, d), ",".join(TARGETS), 600, *extra)
    print(open(os.path.join(out, "bake", "bake.json")).read())

style = os.path.join(out, "style.json")
blend = os.path.join(out, "bake", "baked.blend")
if "still" in steps:
    tools.blender("relight.py", os.path.join(out, "bake"), style, out, "still", blend=blend)
    tools.blend(concept, os.path.join(out, "still.png"), os.path.join(out, "still-vs-concept.png"))
if "tests" in steps:
    tools.blender("relight.py", os.path.join(out, "bake"), style, os.path.join(out, "tests"), "tests", blend=blend)
    tools.blender("relight.py", os.path.join(out, "bake-raw"), style, os.path.join(out, "tests-raw"), "tests",
                  blend=os.path.join(out, "bake-raw", "baked.blend"))
    names = ["rest", "swung", "lamp-off", "porthole-off", "swung-side"]
    tools.contact_sheet([("delit", [(n, os.path.join(out, "tests", f"test-{n}.png")) for n in names]),
                         ("raw paint", [(n, os.path.join(out, "tests-raw", f"test-{n}.png")) for n in names])],
                        os.path.join(out, "tests-sheet.png"), cell_w=320)
if "anim" in steps:
    tools.blender("relight.py", os.path.join(out, "bake"), style, out, "anim", blend=blend)
    for cam in ("concept", "side"):
        d = os.path.join(out, "anim-" + cam)
        frames = sorted(os.path.join(d, f) for f in os.listdir(d) if f.endswith(".png"))
        tools.gif(frames, os.path.join(out, f"anim-{cam}.gif"), ms=42, width=540)
        tools.strip(frames, os.path.join(out, f"strip-{cam}.png"), n=8, width=270)
print("ok", out)
