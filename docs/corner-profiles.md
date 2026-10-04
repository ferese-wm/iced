# Corner profiles

`Border` and raster `Image` carry a `Shape`: `Circular` is the default;
`Continuous` selects Ferese's three-cubic squircle profile. Radius values remain
lengths. Zero-radius corners, circles and pills retain their ordinary geometry.

```rust
use iced::border::{Border, Shape};

let border = Border::default().rounded(12).shape(Shape::Continuous);
```

The profile comes from [ferese-shape](https://github.com/ferese-wm/ferese-shape).
Its measured controls have a small tangent discontinuity at the segment joins.
The name `Continuous` distinguishes this profile from circular corners; it does
not promise mathematical tangent or curvature continuity.

## Borders and inset surfaces

An `Outline` retains the original bounds, normalized corner radii and shape.
Calling `inset` accumulates a distance from that original contour. Borders use
an offset of the same signed distance. Rebuilding a smaller squircle with a
smaller radius does not give the same contour.

Attach an explicit outline with `Border::outline` or `Image::outline` when a
surface must follow its parent's contour. Supply the outline in the same logical
coordinates as the draw bounds. Renderer transforms apply to both once.
Ordinary child buttons keep their own outline.

`Path::outline(outline, tolerance)` and `Builder::outline` sample the reference
contour for canvas drawing. To request a quarter-physical-pixel tolerance, divide
`0.25` by the destination scale. Sampling checks several points on each chord;
the fixture tests measure the error against an independent curve reference.
This is an approximation, not a universal error proof. A collapsed inset gives
an empty path. Invalid inputs or exhausted sampling limits return `None`.

## Clipping child content

A rounded background does not clip its children. Clipping is explicit:

- `Container::clip_to_border(true)` clips children to the border's inner contour.
- `Container::clip_outline(outline)` uses the supplied reference contour.
- `Renderer::with_shaped_layer(bounds, outline, draw)` masks a reference contour.
- `Renderer::with_border_layer(bounds, border, snap, inset, draw)` resolves a local
  contour after transforms and pixel snapping.

Automatic border clips use the same snapped bounds and requested radii as the
background. Explicit reference outlines retain their original geometry.
Containers with shaped clipping use the shared contour path for circular
backgrounds too. When drawing a matching circular background directly, set
`Quad::use_contour` to `true`; ordinary circular quads retain their legacy path.

The container background and border are drawn outside the child group. Shaped
clipping does not apply to separate overlays. Ordinary rectangular layers keep
hard clipping; shaped layers use a one-physical-pixel coverage ramp.

Both renderers compose the group before applying its mask. Nested masks use
combined coverage relative to their parent, so identical nested outlines do not
repeatedly multiply the same antialiasing ramp. Coverage is stored at eight-bit
precision. A primitive's own antialiasing still participates in alpha masking.

Clipping uses an intermediate color surface for each active nesting depth. GPU
surfaces and software pixmaps are reused; masks are cached by geometry, scale
and viewport size. Rectangular-only frames do not allocate these surfaces.
Use clipping for content that needs it, rather than every rounded widget.

## Rendering checks

The `iced_wgpu` tests exercise the GPU and software renderers against the shared
distance model. Device tests are ignored by default because they need an adapter.
To include them on a machine with Vulkan or a software adapter:

```sh
cargo test --release -p iced_wgpu --features image,geometry \
  --test corner_geometry --test corner_quads --test corner_images \
  --test shaped_clipping -- --include-ignored
```
