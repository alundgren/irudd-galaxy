# Shared helpers for the background scripts (turntable, export, reimport check).
import bpy

RIGS = ("Bram_Rig_Neutral", "Bram_Rig_Concept", "Bram_Rig_GoalLit")


def use_rig(scene, which):
    """which: 'neutral' or 'goallit'. Same switching as used for every render in the lab:
    only that rig's lights render, and the goal light uses its own world W_GoalLit."""
    neutral = bpy.data.collections["Bram_Rig_Neutral"]
    for o in neutral.objects:
        if o.type == "LIGHT":
            o.hide_render = which != "neutral"
    bpy.data.collections["Bram_Rig_Concept"].hide_render = True
    bpy.data.collections["Bram_Rig_GoalLit"].hide_render = which != "goallit"
    scene.world = bpy.data.worlds["W_GoalLit" if which == "goallit" else "World"]


def asset_meshes():
    """The meshes that make up the bust as rendered: visible meshes of Bram_Asset."""
    return [o for o in bpy.data.collections["Bram_Asset"].all_objects
            if o.type == "MESH" and not o.hide_render]


def render(scene, camera, path, w, h):
    scene.camera = bpy.data.objects[camera]
    scene.render.resolution_x = w
    scene.render.resolution_y = h
    scene.render.resolution_percentage = 100
    scene.render.use_stamp = False
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True, scene=scene.name)
    return path
