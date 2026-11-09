"""Piece 4 driver: the porthole with a layered, moving space view.
usage: python -I scripts/piece4.py [layers,render]"""
import os, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tools

S = tools.STUDY
p1 = os.path.join(S, "pieces", "01-lamp-corner")
out = os.path.join(S, "pieces", "04-porthole")
layers = os.path.join(out, "layers")
steps = sys.argv[1].split(",") if len(sys.argv) > 1 else ["layers", "render"]
if "layers" in steps:
    subprocess.run([sys.executable, "-I", os.path.join(tools.HERE, "porthole_layers.py"),
                    os.path.join(S, "concept", "room.png"), layers], check=True)
if "render" in steps:
    bake = os.path.join(p1, "bake")
    tools.blender("relight.py", bake, os.path.join(p1, "style.json"), out, "porthole", layers,
                  blend=os.path.join(bake, "baked.blend"))
    for cam in ("dolly", "window"):
        d = os.path.join(out, "anim-" + cam)
        frames = sorted(os.path.join(d, f) for f in os.listdir(d) if f.endswith(".png"))
        tools.gif(frames, os.path.join(out, f"anim-{cam}.gif"), ms=42, width=540)
        tools.strip(frames, os.path.join(out, f"strip-{cam}.png"), n=6, width=300)
print("ok", out)
