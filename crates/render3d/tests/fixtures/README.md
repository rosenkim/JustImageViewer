# GLB fixtures

These small assets are original test data, covered by the repository license.
Run `python3 crates/render3d/tests/fixtures/generate_glb.py` to regenerate them using only the Python standard library.

- `textured.glb`: one triangle with all five PBR texture slots.
- `transformed.glb`: nested translation and scale.
- `morph.glb`: an initial morph weight of 0.5.
- `skin.glb`: a translated joint in the initial pose.
- `transparent.glb`: alpha blending.
- `image.png`: a one-pixel raster image for mixed media and thumbnail padding tests.
- `cube.glb`: a six-sided model for camera and native viewer checks.

Tests check geometry, pixels, errors, and GPU readback. They do not assert byte-for-byte rendered images across GPU vendors.
