"""Piece 3 driver: split-flap arrivals board from the concept's own slot paint.
usage: python -I scripts/piece3.py"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tools

S = tools.STUDY
p1 = os.path.join(S, "pieces", "01-lamp-corner")
out = os.path.join(S, "pieces", "03-board")
os.makedirs(out, exist_ok=True)
bake = os.path.join(p1, "bake")
tools.blender("relight.py", bake, os.path.join(p1, "style.json"), out, "board", blend=os.path.join(bake, "baked.blend"))
for cam in ("concept", "board"):
    d = os.path.join(out, "anim-" + cam)
    frames = sorted(os.path.join(d, f) for f in os.listdir(d) if f.endswith(".png"))
    tools.gif(frames, os.path.join(out, f"anim-{cam}.gif"), ms=42, width=540)
    tools.strip(frames[16:64], os.path.join(out, f"strip-{cam}.png"), n=8, width=270)
print("ok", out)
