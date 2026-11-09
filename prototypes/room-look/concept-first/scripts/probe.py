import bpy, sys
r = bpy.context.scene.render
print("ENGINES", [i.identifier for i in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items])
print("HAS_FREESTYLE", hasattr(r, "use_freestyle"))
print("FILE_FORMATS", [i.identifier for i in bpy.types.ImageFormatSettings.bl_rna.properties["file_format"].enum_items])
try:
    prefs = bpy.context.preferences.addons["cycles"].preferences
    for t in ("OPTIX", "CUDA"):
        try:
            prefs.compute_device_type = t
            prefs.get_devices()
            print("DEV", t, [(d.name, d.type) for d in prefs.devices])
        except Exception as e:
            print("DEVERR", t, e)
except Exception as e:
    print("CYCLESPREF", e)
print("GP types", [t for t in dir(bpy.types) if "GreasePencil" in t][:8])
print("LINEART", "LineartGpencilModifier" in dir(bpy.types), "GreasePencilLineartModifier" in dir(bpy.types))
