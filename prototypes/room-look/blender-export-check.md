# Blender export comparison

Use this check on the finished board and, if attempted, the bust. Keep the source
Blender file and imported export in separate scenes so a reimport cannot overwrite
editable work.

The [Blender glTF documentation](https://docs.blender.org/manual/en/latest/addons/scene_gltf2.html)
describes supported material inputs. The exporter recognises selected node patterns,
including Principled base colour and image textures. Arbitrary procedural node
networks do not transfer as equivalent runtime shaders. Bake necessary authored
material detail into image maps and record the conversion.

For a comparable result:

- Preserve camera, object scale, light setup, exposure and colour-management settings.
- Capture the authored object and the reimported export under those same conditions.
- Check geometry, normals, colours, fine marks and material differences. Inspect an
  oblique view as well as the reference camera.
- List effects present only in the Blender source render. These cannot count as
  exported-asset successes.
- Check texture dimensions and memory, embedded or relative dependencies, and the
  export's material extensions. Retain intentional source detail at useful size.
- For motion, verify the exported clips or expression data on the imported model.
  A moving source model does not prove its export is animated.

Colour comparison starts with an unlit or otherwise controlled diagnostic, followed
by the intended lit appearance. Record the view transform explicitly; changing
exposure or tone mapping between captures can obscure a material mismatch.

A successful reimport proves a standard asset can preserve the tested content in
Blender. It does not establish current Galaxy renderer support or real-time frame
pacing. The morning report must state these limits beside the evidence.
