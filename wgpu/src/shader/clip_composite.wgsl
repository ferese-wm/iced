@group(0) @binding(0) var content: texture_2d<f32>;
@group(0) @binding(1) var mask: texture_2d<f32>;
@group(0) @binding(2) var parent_mask: texture_2d<f32>;
@group(0) @binding(3) var<uniform> nested: vec4<u32>;

@vertex
fn composite_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

@fragment
fn composite_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let coverage = textureLoad(mask, pixel, 0).r;
    var parent = 1.0;

    if nested.x != 0u {
        parent = textureLoad(parent_mask, pixel, 0).r;
    }

    // The parent group applies its own mask later. This ratio avoids squaring
    // the same antialiasing ramp when nested outlines share an edge.
    var relative = 0.0;

    if parent > 0.0 {
        relative = clamp(coverage / parent, 0.0, 1.0);
    }

    return textureLoad(content, pixel, 0) * relative;
}
