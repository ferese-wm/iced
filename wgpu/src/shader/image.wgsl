struct Globals {
    transform: mat4x4<f32>,
    scale_factor: f32,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var u_sampler: sampler;
@group(1) @binding(0) var u_texture: texture_2d_array<f32>;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) center: vec2<f32>,
    @location(1) clip_bounds: vec4<f32>,
    @location(2) border_radius: vec4<f32>,
    @location(3) tile: vec4<f32>,
    @location(4) rotation: f32,
    @location(5) opacity: f32,
    @location(6) atlas_pos: vec2<f32>,
    @location(7) atlas_scale: vec2<f32>,
    @location(8) layer: i32,
    @location(9) snap: u32,
    @location(10) outline_bounds: vec4<f32>,
    @location(11) outline_inset: f32,
    @location(12) shape_and_contour: vec2<u32>,
    @location(13) image_bounds: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) clip_bounds: vec4<f32>,
    @location(1) @interpolate(flat) border_radius: vec4<f32>,
    @location(2) @interpolate(flat) atlas: vec4<f32>,
    @location(3) @interpolate(flat) layer: i32,
    @location(4) @interpolate(flat) opacity: f32,
    @location(5) uv: vec2<f32>,
    @location(6) @interpolate(flat) outline_bounds: vec4<f32>,
    @location(7) @interpolate(flat) outline_inset: f32,
    @location(8) @interpolate(flat) shape_and_contour: vec2<u32>,
    @location(9) @interpolate(flat) image_bounds: vec4<f32>,
    @location(10) unrotated: vec2<f32>,
    @location(11) @interpolate(flat) tile: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Generate a vertex position in the range [0, 1] from the vertex index
    let corner = vertex_position(input.vertex_index);

    let tile = input.tile;
    let center = input.center;

    // List the unrotated tile corners
    let corners = array<vec2<f32>, 4>(
        tile.xy,                              // Top left
        tile.xy + vec2<f32>(tile.z, 0.0),     // Top right
        tile.xy + vec2<f32>(0.0, tile.w),     // Bottom left
        tile.xy + tile.zw                     // Bottom right
    );

    // Rotate tile corners around center
    let cos_r = cos(-input.rotation); // Clockwise
    let sin_r = sin(-input.rotation);
    var rotated = array<vec2<f32>, 4>();

    for (var i = 0u; i < 4u; i++) {
        let c = corners[i] - input.center;
        rotated[i] = vec2<f32>(c.x * cos_r - c.y * sin_r, c.x * sin_r + c.y * cos_r) + input.center;
    }

    // Find bounding box of rotated tile
    var min_xy = rotated[0];
    var max_xy = rotated[0];
    for (var i = 1u; i < 4u; i++) {
        min_xy = min(min_xy, rotated[i]);
        max_xy = max(max_xy, rotated[i]);
    }
    let rotated_bounds = vec4<f32>(min_xy, max_xy - min_xy);

    // Intersect with clip bounds
    let margin = select(0.0, 0.5 / globals.scale_factor, input.shape_and_contour.y != 0u);
    let clip_min = max(rotated_bounds.xy - margin, input.clip_bounds.xy - margin);
    let clip_max = min(rotated_bounds.xy + rotated_bounds.zw + margin, input.clip_bounds.xy + input.clip_bounds.zw + margin);
    let clipped_tile = vec4<f32>(clip_min, max(vec2<f32>(0.0), clip_max - clip_min));

    // Calculate the vertex position
    let v_pos = clipped_tile.xy + corner * clipped_tile.zw;
    out.position = vec4(vec2(globals.scale_factor), 1.0, 1.0) * vec4<f32>(v_pos, 0.0, 1.0);
    out.clip_bounds = globals.scale_factor * input.clip_bounds;

    // Rotate in destination coordinates before converting to atlas UVs.
    // Rotating UV coordinates directly distorts non-square images and tiles.
    let delta = v_pos - center;
    let unrotated = vec2(delta.x * cos_r + delta.y * sin_r, -delta.x * sin_r + delta.y * cos_r) + center;
    out.uv = input.atlas_pos + (unrotated - tile.xy) / tile.zw * input.atlas_scale;
    out.unrotated = unrotated * globals.scale_factor;
    out.image_bounds = input.image_bounds * globals.scale_factor;
    out.tile = tile * globals.scale_factor;

    // Snap position to the pixel grid
    if bool(input.snap) {
        out.position = round(out.position);
        out.clip_bounds = vec4(
            round(out.clip_bounds.xy),
            round(out.clip_bounds.xy + out.clip_bounds.zw) - out.clip_bounds.xy,
        );
    }

    out.position = globals.transform * out.position;
    out.border_radius = globals.scale_factor * min(input.border_radius, vec4(min(input.clip_bounds.z, input.clip_bounds.w) / 2.0));
    out.outline_bounds = input.outline_bounds * globals.scale_factor;
    out.outline_inset = input.outline_inset * globals.scale_factor;
    out.shape_and_contour = input.shape_and_contour;

    if input.shape_and_contour.y != 0u {
        out.border_radius = input.border_radius * globals.scale_factor;
    }

    out.atlas = vec4(input.atlas_pos, input.atlas_pos + input.atlas_scale);
    out.layer = input.layer;
    out.opacity = input.opacity;

    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let fragment = input.position.xy;
    let position = input.clip_bounds.xy;
    let scale = input.clip_bounds.zw;

    let d = rounded_box_sdf(
        2.0 * (fragment - position - scale / 2.0),
        scale,
        input.border_radius * 2.0,
    ) / 2.0;

    var antialias = clamp(1.0 - d, 0.0, 1.0);

    if input.shape_and_contour.y != 0u {
        let contour_distance = shape_distance(fragment, input.outline_bounds, input.border_radius, input.shape_and_contour.x, input.outline_inset);
        let clip_distance = shape_distance(fragment, input.clip_bounds, vec4(0.0), 0u, 0.0);
        let image_distance = shape_distance(input.unrotated, input.image_bounds, vec4(0.0), 0u, 0.0);
        antialias = shape_coverage(max(max(contour_distance, clip_distance), image_distance));
    }
    var inside = all(input.uv >= input.atlas.xy) && all(input.uv <= input.atlas.zw);
    var uv = input.uv;

    if input.shape_and_contour.y != 0u {
        let lower = input.uv >= input.atlas.xy;
        let upper = input.uv <= input.atlas.zw;
        let external_lower = abs(input.tile.xy - input.image_bounds.xy) < vec2(0.001);
        let external_upper = abs(input.tile.xy + input.tile.zw - input.image_bounds.xy - input.image_bounds.zw) < vec2(0.001);
        inside = all(lower | external_lower) && all(upper | external_upper);
        let half_texel = vec2(0.5) / vec2<f32>(textureDimensions(u_texture));
        uv = clamp(uv, input.atlas.xy + half_texel, input.atlas.zw - half_texel);
    }

    return textureSample(u_texture, u_sampler, uv, input.layer) * vec4<f32>(1.0, 1.0, 1.0, antialias * input.opacity * f32(inside));
}

fn rounded_box_sdf(p: vec2<f32>, size: vec2<f32>, corners: vec4<f32>) -> f32 {
    var box_half = select(corners.yz, corners.xw, p.x > 0.0);
    var corner = select(box_half.y, box_half.x, p.y > 0.0);
    var q = abs(p) - size + corner;
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - corner;
}
