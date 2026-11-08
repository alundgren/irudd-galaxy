# Board checkpoint 4: coordinator inspection

The painted ship, interrupted highlights and local wear improve the asset. The
turntable contact sheet shows the marks stay attached across the sampled views.
It does not establish playback smoothness or runtime frame rate.

Provisional look adherence and graphical quality remain 3/5. Three visible issues
still dominate the bounded reference comparison:

- Fastener centres are dark. The reference has bright metal centres inside dark
  surrounds. Improve their material/light response and deliberate painted accents.
- The plaque's cream rim strokes are too weak, especially the left and lower edge.
  Add narrow, broken strokes with local rust alongside them. Keep the large cream
  field calm and avoid a uniformly bright perimeter.
- The bracket's inner corner is too square and its upper arm too straight. Refine
  that local contour towards the reference's rounded transition and slight taper.
  Keep a coherent object in oblique views rather than matching only a camera image.

The top rail also needs a small local brightness correction near the lamp, without
raising exposure or brightening the plaque. Do not expand the prop or add general
noise. Retain the editable paint layer and current improved construction.

The exported GLB was independently inspected: 18 meshes, 15 materials, one embedded
PNG, no external buffer dependency, and KHR_materials_specular. The outline material
is single-sided by glTF default. This structural inspection does not replace the
render comparison or a runtime test. The reimport script changes shadow backface
culling for the outline material; describe that exact setting in the handoff rather
than suggesting glTF contains a portable no-shadow flag.

Apply these focused corrections before independent asset review round 2. Round 1's
four findings remain the review scope; this checkpoint does not consume a new
independent review round.
