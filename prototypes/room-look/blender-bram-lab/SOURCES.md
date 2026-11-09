# Outside sources used in the Bram lab

## Human Base Meshes asset bundle (Blender Studio and community)

- Listed on: https://www.blender.org/download/demo-files/ (section "Asset Bundles":
  "Human Base Meshes v1.4.1 by Blender Studio and community contributions. 49 MB – CC0").
- Downloaded from the official host, over HTTPS, without an account:
  https://download.blender.org/demo/asset-bundles/human-base-meshes/human-base-meshes-bundle-v1.4.1.zip
  (directory listing date 20-Jan-2026, 50,643,039 bytes).
- SHA-256 of the zip: `811f43accbb31a88266d932f8f5563b2d13586fca0ba2693aad1f5fe582b3515`
- SHA-256 of `human_base_meshes_bundle.blend` inside it:
  `3c121505651140ceb4d69fd1d8923f7788ffadd81672f5be14845a5f2c75c137`
- The zip holds that .blend, `blender_assets.cats.txt` and thumbnails. It was opened only
  with `--disable-autoexec` in a background Blender; no script inside it was run.

### License evidence

- The bundle's README text block: "All provided assets are public domain under the CC0
  license."
- The asset metadata of the asset used here, collection "Head (Animation) - Realistic":
  license `CC0`, author `Dan Ulrich`, description "Realistic scan data model with
  multiresolution details. Topology optimized for r[igging]...".
- The bundle also carries a text block "License" that says "The Rain Rig is released under
  the Creative Commons Attribution 4.0 license". It refers to the Rain character that the
  stylized female body is based on. That asset is not used here.

### What was taken and how it was changed

- Taken: the mesh `GEO-head_animation_realistic` (3,242 base vertices, one Multires level,
  UV map, face sets) and its eye parts (`.sclera.L/R`, `.iris.L/R`). It had no drivers or
  animation; none were copied.
- Kept unmodified in `bram_bust.blend` as the hidden object `HBM_Head_Original` (with its
  eye parts), for comparison.
- Changes made for Bram are listed in `README.md` under "What the source contains". The
  exported `bram_bust.glb` contains the modified head (not the hidden original).
