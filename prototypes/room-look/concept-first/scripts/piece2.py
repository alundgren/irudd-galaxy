"""Piece 2 driver: fill what the concept never showed. Re-bakes piece 1's objects (frame fix),
keeps the smooth-fill version, runs fill_by_example (detail mode), renders both from several cameras.
usage: python -I scripts/piece2.py [bake,fill,views]"""
import os, shutil, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tools

S = tools.STUDY
p1 = os.path.join(S, "pieces", "01-lamp-corner")
out = os.path.join(S, "pieces", "02-unseen-fill")
os.makedirs(out, exist_ok=True)
bake = os.path.join(p1, "bake")
smooth_dir = os.path.join(out, "bake-smooth")
FILL = ["wall_back", "board_frame", "lamp_shade", "lamp_arm_a", "lamp_arm_b", "arches", "pillar_teal",
        "wall_left", "porthole_tube", "porthole_bezel",
        "desk_top", "desk_front", "book", "sign", "mug", "rack", "case_low", "case_high", "plant_pot",
        "plant_leaves", "bram_head", "bram_cap", "bram_torso", "chair_back"]
steps = sys.argv[1].split(",") if len(sys.argv) > 1 else ["bake", "fill", "views"]

if "bake" in steps:
    for f in os.listdir(bake):
        if f.endswith("-smooth.png") or f.endswith("-fillmap.png"):
            os.remove(os.path.join(bake, f))
    subprocess.run([sys.executable, "-I", os.path.join(tools.HERE, "piece1.py"), "bake"], check=True)
    if os.path.exists(smooth_dir):
        shutil.rmtree(smooth_dir)
    shutil.copytree(bake, smooth_dir)
if "fill" in steps:
    subprocess.run([sys.executable, "-I", os.path.join(tools.HERE, "fill_by_example.py"), bake, ",".join(FILL), "48", "1", "detail"],
                   check=True)
if "views" in steps:
    style = os.path.join(p1, "style.json")
    blend = os.path.join(bake, "baked.blend")
    for label, d in (("smooth", smooth_dir), ("detail", bake)):
        tools.blender("relight.py", d, style, os.path.join(out, label), "views", blend=blend)
    views = ["concept", "yaw-25", "yaw+25", "yaw+50", "high"]
    tools.contact_sheet([(lab, [(v, os.path.join(out, lab, f"view-{v}.png")) for v in views]) for lab in ("smooth", "detail")],
                        os.path.join(out, "views-sheet.png"), cell_w=320)
print("ok", out)
