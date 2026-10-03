struct Globals {
    transform: mat4x4<f32>,
    scale: f32,
}

@group(0) @binding(0) var<uniform> globals: Globals;

fn rounded_box_sdf(p: vec2<f32>, size: vec2<f32>, corners: vec4<f32>) -> f32 {
    var box_half = select(corners.yz, corners.xw, p.x > 0.0);
    var corner = select(box_half.y, box_half.x, p.y > 0.0);
    var q = abs(p) - size + corner;
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - corner;
}

// A reference inset changes distance, not the original curve's radii.
fn contour_quad_color(
    point: vec2<f32>, bounds: vec4<f32>, reference: vec4<f32>,
    radii: vec4<f32>, kind: u32, inset: f32, width: f32,
    fill: vec4<f32>, border: vec4<f32>, shadow: vec4<f32>,
    shadow_offset: vec2<f32>, shadow_blur: f32,
) -> vec4<f32> {
    let box_distance = shape_distance(point, bounds, vec4(0.0), 0u, 0.0);
    let distance = max(box_distance, shape_distance(point, reference, radii, kind, inset));
    let outer = shape_coverage(distance);
    let inner = shape_coverage(distance + max(width, 0.0));
    let color = fill * inner + border * (outer - inner);

    if shadow.a <= 0.0 {
        return color;
    }

    let shadow_point = point - shadow_offset;
    let shadow_distance = max(
        shape_distance(shadow_point, bounds, vec4(0.0), 0u, 0.0),
        shape_distance(shadow_point, reference, radii, kind, inset),
    );
    var shadow_alpha = shape_coverage(shadow_distance);

    if shadow_blur > 0.0 {
        shadow_alpha = 1.0 - smoothstep(-shadow_blur, shadow_blur, max(shadow_distance, 0.0));
    }

    return color + shadow * shadow_alpha * (1.0 - outer);
}
