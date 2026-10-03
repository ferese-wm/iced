struct Contour {
    bounds: vec4<f32>,
    radii: vec4<f32>,
    clip: vec4<f32>,
    style: vec4<u32>,
}

struct ClipUniforms {
    count: vec4<u32>,
    contours: array<Contour, 16>,
}

@group(0) @binding(0) var<uniform> clips: ClipUniforms;

@vertex
fn mask_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

@fragment
fn mask_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    var coverage = 1.0;

    for (var i = 0u; i < clips.count.x; i++) {
        let contour = clips.contours[i];

        if contour.style.z == 0u {
            return vec4(0.0);
        }

        let clip_distance = shape_distance(position.xy, contour.clip, vec4(0.0), 0u, 0.0);

        if clip_distance >= 0.5 {
            return vec4(0.0);
        }

        let distance = max(
            shape_distance(position.xy, contour.bounds, contour.radii, contour.style.x, bitcast<f32>(contour.style.y)),
            clip_distance,
        );
        coverage = min(coverage, shape_coverage(distance));
    }

    return vec4(coverage);
}
