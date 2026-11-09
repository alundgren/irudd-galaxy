"""Whole recipe in order, for a changed blockout (layout.py) or a new concept.
  piece0  ID render, palette, unseen measure
  piece1  delight + measure the painter's light; first bake (guard from the blockout)
  matte   difference matte: painted silhouette of the foreground; piece1 delight again with it
  piece2  second bake (guard from the matte, foreground alphas) + unseen fill
  piece4  porthole space layers cut from the concept
  piece5  whole-room animation
usage: python -I scripts/room.py"""
import os, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tools

p1 = os.path.join(tools.STUDY, "pieces", "01-lamp-corner")


def run(script, *a):
    print("==", script, *a, flush=True)
    subprocess.run([sys.executable, "-I", os.path.join(tools.HERE, script), *a], check=True)


run("piece0.py")
for f in ("matte.png", "guard-matte.png", "id-painted.png"):  # first bake must not use a matte from an older blockout
    if os.path.exists(os.path.join(p1, f)):
        os.remove(os.path.join(p1, f))
run("piece1.py", "delight,bake")
run("matte.py")
run("piece1.py", "delight")          # again, with the painted silhouettes
run("piece2.py", "bake,fill")
run("piece4.py", "layers")           # porthole space layers (sky plate, tower, platform) for the room job
run("piece5.py", "render,sheets")
print("ok room")
