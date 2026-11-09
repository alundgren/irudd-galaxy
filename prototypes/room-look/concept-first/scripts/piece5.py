"""Piece 5 driver: the whole room through the recipe at once (lamp, board, porthole, Bram, desk).
Assumes piece2 bake,fill ran with the full ROOM target list (piece1.TARGETS) and piece4 layers exist.
usage: python -I scripts/piece5.py [render,sheets]"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tools

S = tools.STUDY
p1 = os.path.join(S, "pieces", "01-lamp-corner")
bake = os.path.join(p1, "bake")
out = os.path.join(S, "pieces", "05-room")
layers = os.path.join(S, "pieces", "04-porthole", "layers")
os.makedirs(out, exist_ok=True)
steps = sys.argv[1].split(",") if len(sys.argv) > 1 else ["render", "sheets"]
if "render" in steps:
    tools.blender("relight.py", bake, os.path.join(p1, "style.json"), out, "room", layers,
                  blend=os.path.join(bake, "baked.blend"))
if "export" in steps:
    # board fitted into the partner's recess (opening W,H centred at x,z, front face y), porthole on its own root
    tools.blender("relight.py", bake, os.path.join(p1, "style.json"), os.path.join(S, "export"), "export", layers,
                  "1.06,0.74,0.89,1.33,0.755", blend=os.path.join(bake, "baked.blend"))
if "sheets" in steps:
    for cam in ("push", "side"):
        d = os.path.join(out, "anim-" + cam)
        frames = sorted(os.path.join(d, f) for f in os.listdir(d) if f.endswith(".png"))
        tools.gif(frames, os.path.join(out, f"anim-{cam}.gif"), ms=42, width=540)
        tools.strip(frames, os.path.join(out, f"strip-{cam}.png"), n=8, width=270)
print("ok", out)
