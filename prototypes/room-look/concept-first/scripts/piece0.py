"""Piece 0 driver: camera match + blockout + concept palette.  usage: python -I scripts/piece0.py"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tools

out = os.path.join(tools.STUDY, "pieces", "00-layout")
os.makedirs(out, exist_ok=True)
concept = os.path.join(tools.STUDY, "concept", "room.png")

tools.blender("piece0_blender.py", "id", os.path.join(out, "id.png"))
pal, cover = tools.palette_from_id(os.path.join(out, "id.png"), os.path.join(out, "id.json"), concept)
json.dump(pal, open(os.path.join(out, "palette.json"), "w"), indent=1)
json.dump(cover, open(os.path.join(out, "coverage.json"), "w"), indent=1)
tools.blender("piece0_blender.py", "views", os.path.join(out, "palette.json"), concept, out)
tools.blend(concept, os.path.join(out, "palette-concept.png"), os.path.join(out, "overlay-blend.png"))
tools.edge_overlay(concept, os.path.join(out, "palette-concept.png"), os.path.join(out, "overlay-edges.png"))
views = ["concept", "yaw-25", "yaw+25", "yaw+50", "high"]
tools.contact_sheet([(lab, [(v, os.path.join(out, f"{lab}-{v}.png")) for v in views])
                     for lab in ("projected", "palette", "lit")], os.path.join(out, "sheet.png"), cell_w=300)
unseen = {v: tools.unseen_fraction(os.path.join(out, f"projected-{v}.png")) for v in views}
json.dump(unseen, open(os.path.join(out, "unseen.json"), "w"), indent=1)
print("unseen fraction per view:", unseen)
print("ok", out)
