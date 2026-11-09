"""Probe: does the torso's alpha make it see-through in EEVEE? Prints material settings after relight's
setup and renders the torso alone over a green background with three render methods.
usage: blender -b baked.blend ... --python probe_alpha.py -- <bake_dir> <style.json> <outdir>"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy
import cf

bake_dir, style, outdir = cf.argv()[:3]
sys.argv = [sys.argv[0], "--", bake_dir, style, outdir, "probe", "bram_torso,lamp_shade"]
exec(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "relight.py"), encoding="utf-8").read())
ob = bpy.data.objects["bram_torso"]
m = ob.active_material
print("PROBE", m.name, "method", getattr(m, "surface_render_method", None),
      "nodes", [n.type for n in m.node_tree.nodes])
sc = bpy.context.scene
sc.render.use_freestyle = False
sc.world.color = (0, 1, 0)
for meth in ("DITHERED", "BLENDED"):
    try:
        m.surface_render_method = meth
    except Exception as e:
        print("PROBE fail", meth, e)
        continue
    cf.render(sc, ref_cam, os.path.join(outdir, f"probe-{meth}.png"))
