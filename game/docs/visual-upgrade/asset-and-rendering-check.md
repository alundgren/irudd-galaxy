# Asset and rendering proofs

Use this method for the [incremental visual proofs](../visual-upgrade-plan.md#incremental-visual-proofs).
Test a prop, an adult bust, rendering treatment and deformation separately before
assembling the office. Each has an owner review against the goal image. No
production backend is selected here. Engine documentation describes available
tools; it does not prove the quality or performance of our assets in those tools.

## Separate the work

| Work | What must be authored | What the runtime must preserve |
| --- | --- | --- |
| Character | Adult proportions, cheek/jaw/eyelid/lip volumes, deliberate hair, clothing folds, expression and gesture | Deformation, light, contours, hand contact, selecting the posed being and the speech anchor |
| Surface detail | Painted colour and fine marks, material-specific wear, texture coordinates, useful normal and roughness detail | Stable sampling, filtering, material response and readable detail during motion |
| Room | Framing, proportions, bevels, purposeful props, palette and light placement | Cast shadows, controlled light/fill, camera clearance, readable tasks and UI |

Our diagnosis from the current images is that authored anatomy, hair, clothing and
composition are major remaining weaknesses. The existing lab already demonstrates
outlines, surface variation and a moving lamp shadow. Increasing procedural noise
cannot establish missing facial volumes or a distinctive hairstyle. Fine normal
texture can affect lighting, but changing the outer profile requires geometry.

A mature engine supplies machinery for imports, materials, deformation and lighting.
The model, painted marks, animation and composition still need to be authored and
checked after import. A fully procedural pipeline is not a requirement of this
experiment; any change to the core's existing model policy belongs in the direction
amendment.

## Engine technique references

Engine and authoring-tool names are allowed here; other game names remain excluded.
These are available approaches, not claims that a stock setting reproduces the goal.

- **Unreal.** Custom post-process materials can use depth, normals and object masks
  for selected contours. Pass placement relative to antialiasing and tone mapping
  matters. Materials, assets and lighting still need their own art direction.
  [Official post-process documentation](https://dev.epicgames.com/documentation/en-us/unreal-engine/post-process-materials-in-unreal-engine)
- **Unity.** HDRP Custom Passes and URP render-pass injection provide places to add
  a controlled line treatment. The lighting/material and outline work must be tested
  in the selected pipeline and version.
  [HDRP Custom Pass](https://docs.unity3d.com/Packages/com.unity.render-pipelines.high-definition@17.0/manual/Custom-Pass.html),
  [URP injection points](https://docs.unity.com/en-us/engine/6000.3/manual/render-pipelines/universal-render-pipeline/customizing-urp/inject-a-render-pass/custom-pass-injection-points)
- **Godot.** Spatial shaders include toon diffuse/specular modes; standard materials
  also expose outline options. Those are starting tools, not authored facial ink.
  Transparency and outline intersections need testing. glTF import provides a
  documented route for meshes, materials and animation.
  [Spatial shaders](https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/spatial_shader.html),
  [Standard materials](https://docs.godotengine.org/en/stable/tutorials/3d/standard_material_3d.html),
  [Import formats](https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/available_formats.html)

Treat authoring-tool output and runtime appearance as separate checks. For example,
the documented Blender-to-glTF import route has export settings affecting deformation,
shading and culling. An authoring viewport image alone is not runtime evidence.
Record the chosen version, material conversion and export settings for reproduction.

## Test one cause at a time

1. Start with the selected reference crop and its intended display size. Use the
   goal as the art reference. Earlier prototypes are useful defect reproductions,
   but are too far from the goal to define the next asset's appearance.
2. Identify who or what will author the mesh, texture and later animation. Keep
   editable source assets and record usage rights. A licensed diagnostic asset
   can test import, but does not establish Bram's identity or our look. Any paid
   asset or specialist work needs a separate concrete proposal before spending.
3. Inspect the source asset under plain light. For the board, check construction,
   bevels and intentional marks. For the bust, check adult proportions, facial
   volumes, hair and cloth. If it fails here, revise the asset before adding
   shader detail. If the authoring method cannot reach the target, change that
   method rather than assembling more weak assets into a room.
4. Import actual asset data. Record normals, texture coordinates, image maps and
   later rig, weights, expressions and clips that the runtime preserves. Record
   manual material conversion and unsupported data. An authoring viewport image
   alone is not a runtime pass.
5. Lock framing, pose and lighting for comparisons. Keep plain lighting available.
   Add material response, outlines and shadows individually on the approved asset,
   then test them together. Move the camera and light to expose swimming marks,
   unstable contours and broken shadows. Compare close and intended play sizes.
6. Add deformation only after appearance passes. Start with one facial action,
   then extend to the reach and contact test. Check outlines, shadows, picking
   and speech anchors against the deformed model. Only then combine it with room
   staging and interaction.

Use the existing eight-category scorecard and owner test stops. Mark unrelated
categories N/A. Record import changes, one requested asset correction and the
steps needed to see it in runtime again. Film cadence does not establish 60 Hz.

If a convincing source asset loses paint or deformation in the current renderer,
that identifies a concrete capability gap. When needed, import the same asset into
one candidate runtime to establish a reference. Test the current renderer's
supported subset and list omissions explicitly. Do not build two complete import
and animation systems for this diagnostic.

A good image without working selection, speech anchors and state reconstruction
has not proved a viable interior integration. Estimate that work before choosing
a production backend. Keep the same accepted assets as tests when capabilities
are combined, so a better light does not conceal a regression in skin or ink.
