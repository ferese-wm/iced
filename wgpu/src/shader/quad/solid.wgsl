struct SolidVertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) color: vec4<f32>,
    @location(1) pos: vec2<f32>,
    @location(2) scale: vec2<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) border_radius: vec4<f32>,
    @location(5) border_width: f32,
    @location(6) shadow_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_blur_radius: f32,
    @location(9) snap: u32,
    @location(10) outline_bounds: vec4<f32>,
    @location(11) outline_inset: f32,
    @location(12) shape_and_contour: vec2<u32>,
}

struct SolidVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) border_color: vec4<f32>,
    @location(2) pos: vec2<f32>,
    @location(3) scale: vec2<f32>,
    @location(4) border_radius: vec4<f32>,
    @location(5) border_width: f32,
    @location(6) shadow_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_blur_radius: f32,
    @location(9) outline_bounds: vec4<f32>,
    @location(10) outline_inset: f32,
    @location(11) @interpolate(flat) shape_and_contour: vec2<u32>,
}

@vertex
fn solid_vs_main(input: SolidVertexInput) -> SolidVertexOutput {
    var out: SolidVertexOutput;

    var pos: vec2<f32> = (input.pos + min(input.shadow_offset, vec2<f32>(0.0, 0.0)) - input.shadow_blur_radius) * globals.scale;
    var scale: vec2<f32> = (input.scale + vec2<f32>(abs(input.shadow_offset.x), abs(input.shadow_offset.y)) + input.shadow_blur_radius * 2.0) * globals.scale;

    var pos_snap = vec2<f32>(0.0, 0.0);
    var scale_snap = vec2<f32>(0.0, 0.0);

    if bool(input.snap) {
        pos_snap = round(pos + vec2(0.001, 0.001)) - pos;
        scale_snap = round(pos + scale + vec2(0.001, 0.001)) - pos - pos_snap - scale;
    }

    if input.shape_and_contour.y != 0u && bool(input.snap) {
        let own_pos = input.pos * globals.scale;
        let own_size = input.scale * globals.scale;
        pos_snap = round(own_pos + vec2(0.001)) - own_pos;
        scale_snap = round(own_pos + own_size + vec2(0.001)) - own_pos - pos_snap - own_size;
    }

    var border_radius = input.border_radius;

    if input.shape_and_contour.y == 0u {
        border_radius = min(border_radius, vec4(min(input.scale.x, input.scale.y) / 2.0));
    }

    var transform: mat4x4<f32> = mat4x4<f32>(
        vec4<f32>(scale.x + scale_snap.x + 1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, scale.y + scale_snap.y + 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(pos + pos_snap - vec2<f32>(0.5, 0.5), 0.0, 1.0)
    );

    out.position = globals.transform * transform * vec4<f32>(vertex_position(input.vertex_index), 0.0, 1.0);
    out.color = premultiply(input.color);
    out.border_color = premultiply(input.border_color);
    out.pos = input.pos * globals.scale + pos_snap;
    out.scale = input.scale * globals.scale + scale_snap;
    out.border_radius = border_radius * globals.scale;
    out.border_width = input.border_width * globals.scale;
    out.shadow_color = premultiply(input.shadow_color);
    out.shadow_offset = input.shadow_offset * globals.scale;
    out.shadow_blur_radius = input.shadow_blur_radius * globals.scale;
    out.outline_bounds = input.outline_bounds * globals.scale;
    out.outline_inset = input.outline_inset * globals.scale;
    out.shape_and_contour = input.shape_and_contour;

    if input.shape_and_contour.y == 1u {
        out.outline_bounds = vec4(out.pos, out.scale);
    }

    return out;
}

@fragment
fn solid_fs_main(
    input: SolidVertexOutput
) -> @location(0) vec4<f32> {
    if input.shape_and_contour.y != 0u {
        return contour_quad_color(
            input.position.xy, vec4(input.pos, input.scale), input.outline_bounds,
            input.border_radius, input.shape_and_contour.x, input.outline_inset,
            input.border_width, input.color, input.border_color, input.shadow_color,
            input.shadow_offset, input.shadow_blur_radius,
        );
    }

    var mixed_color: vec4<f32> = input.color;

    var dist = rounded_box_sdf(
        -(input.position.xy - input.pos - input.scale * 0.5) * 2.0,
        input.scale,
        input.border_radius * 2.0
    ) / 2.0;

    if (input.border_width > 0.0) {
        mixed_color = mix(
            input.color,
            input.border_color,
            clamp(0.5 + dist + input.border_width, 0.0, 1.0)
        );
    }

    var quad_alpha: f32 = clamp(0.5-dist, 0.0, 1.0);

    let quad_color = mixed_color * quad_alpha;

    if input.shadow_color.a > 0.0 {
        var shadow_dist: f32 = rounded_box_sdf(
            -(input.position.xy - input.pos - input.shadow_offset - input.scale/2.0) * 2.0,
            input.scale,
            input.border_radius * 2.0
        ) / 2.0;
        let shadow_alpha = 1.0 - smoothstep(-input.shadow_blur_radius, input.shadow_blur_radius, max(shadow_dist, 0.0));

        return mix(quad_color, input.shadow_color, (1.0 - quad_alpha) * shadow_alpha);
    } else {
        return quad_color;
    }
}
