use crate::core::border::Shape;
use crate::core::renderer::Quad;
use crate::core::shape::{
    Outline, blend, controls, corner_extent, edge_coverage,
};
use crate::core::{Background, Gradient, Rectangle, Transformation};
use crate::engine::into_color;

use std::collections::VecDeque;
use std::sync::Arc;

const CACHE_BYTES: usize = 16 * 1024 * 1024;
// A large contour now occupies up to four perimeter entries.
const CACHE_ENTRIES: usize = 32 * 4;

#[derive(Debug, Clone, PartialEq)]
struct Key {
    outline: Outline,
    bounds: [f64; 4],
    size: [u32; 2],
    width: f64,
    shadow_offset: [f64; 2],
    blur: f64,
    shadow: bool,
}

#[derive(Debug)]
struct Masks {
    key: Key,
    coverage: Vec<[u8; 3]>,
}

#[derive(Debug, Default)]
pub(crate) struct Pipeline {
    cache: VecDeque<Arc<Masks>>,
    bytes: usize,
}

impl Pipeline {
    pub(crate) fn draw(
        &mut self,
        quad: &Quad,
        background: &Background,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &tiny_skia::Mask,
        clip_bounds: Rectangle,
    ) {
        if matches!(background, Background::Color(color) if color.a <= 0.0)
            && (quad.border.width <= 0.0 || quad.border.color.a <= 0.0)
            && quad.shadow.color.a <= 0.0
        {
            return;
        }

        let scale = transformation.scale_factor();
        let translation = transformation.translation();
        let mut bounds = quad.bounds * transformation;
        let Some(mut outline) =
            quad.border.outline_for(quad.bounds).and_then(|o| {
                o.transformed(
                    [translation.x as f64, translation.y as f64],
                    scale as f64,
                )
            })
        else {
            return;
        };

        if quad.snap {
            let position =
                [(bounds.x + 0.001).round(), (bounds.y + 0.001).round()];
            let right = (bounds.x + bounds.width + 0.001).round();
            let bottom = (bounds.y + bounds.height + 0.001).round();
            bounds = Rectangle {
                x: position[0],
                y: position[1],
                width: right - position[0],
                height: bottom - position[1],
            };

            if quad.border.outline.is_none() {
                let Some(snapped) = Outline::new(
                    [bounds.x, bounds.y, bounds.width, bounds.height]
                        .map(f64::from),
                    <[f32; 4]>::from(quad.border.radius)
                        .map(|r| f64::from(r * scale)),
                    outline.shape(),
                ) else {
                    return;
                };
                outline = snapped;
            }
        }

        if quad.border.outline.is_none()
            && outline.radii() == [0.0; 4]
            && quad.border.width <= 0.0
            && quad.shadow.color.a <= 0.0
            && [bounds.x, bounds.y, bounds.width, bounds.height]
                .into_iter()
                .all(|v| v.fract() == 0.0)
            && let Background::Color(color) = background
            && let Some(rect) = tiny_skia::Rect::from_xywh(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
            )
        {
            // Aligned straight edges have binary coverage. Draw the solid
            // rectangle directly instead of constructing an intermediate mask.
            pixels.fill_rect(
                rect,
                &tiny_skia::Paint {
                    shader: tiny_skia::Shader::SolidColor(into_color(*color)),
                    anti_alias: false,
                    ..Default::default()
                },
                tiny_skia::Transform::identity(),
                clipping_mask(
                    clip_mask,
                    [bounds.x.max(0.0) as u32, bounds.y.max(0.0) as u32],
                    [
                        bounds.width.max(0.0) as u32,
                        bounds.height.max(0.0) as u32,
                    ],
                ),
            );
            return;
        }

        let shadow_offset = [
            quad.shadow.offset.x as f64 * scale as f64,
            quad.shadow.offset.y as f64 * scale as f64,
        ];
        let blur = f64::from((quad.shadow.blur_radius * scale).max(0.0));
        let shadow = quad.shadow.color.a > 0.0;
        let extent = if shadow { blur + 1.0 } else { 1.0 };
        let offset = if shadow { shadow_offset } else { [0.0; 2] };
        let left = (bounds.x as f64 + offset[0].min(0.0) - extent)
            .floor()
            .max(clip_bounds.x.floor() as f64)
            .max(0.0) as u32;
        let top = (bounds.y as f64 + offset[1].min(0.0) - extent)
            .floor()
            .max(clip_bounds.y.floor() as f64)
            .max(0.0) as u32;
        let right = (bounds.x as f64
            + bounds.width as f64
            + offset[0].max(0.0)
            + extent)
            .ceil()
            .max(0.0)
            .min(pixels.width() as f64)
            .min((clip_bounds.x + clip_bounds.width).ceil() as f64)
            as u32;
        let bottom = (bounds.y as f64
            + bounds.height as f64
            + offset[1].max(0.0)
            + extent)
            .ceil()
            .max(0.0)
            .min(pixels.height() as f64)
            .min((clip_bounds.y + clip_bounds.height).ceil() as f64)
            as u32;

        if left >= right || top >= bottom {
            return;
        }

        let region = Rectangle {
            x: left as f32,
            y: top as f32,
            width: (right - left) as f32,
            height: (bottom - top) as f32,
        };

        if !region.intersects(&clip_bounds) {
            return;
        }

        let Some(outline) =
            outline.transformed([-(left as f64), -(top as f64)], 1.0)
        else {
            return;
        };
        let key = Key {
            outline,
            bounds: [
                bounds.x as f64 - left as f64,
                bounds.y as f64 - top as f64,
                bounds.width as f64,
                bounds.height as f64,
            ],
            size: [right - left, bottom - top],
            width: f64::from((quad.border.width * scale).max(0.0)),
            shadow_offset,
            blur,
            shadow,
        };
        if let Some([x0, y0, x1, y1]) = interior(&key) {
            // Interior coverage is exactly one. Only the perimeter needs
            // distance masks and an intermediate RGBA image.
            for [x, y, right, bottom] in [
                [0, 0, key.size[0], y0],
                [0, y1, key.size[0], key.size[1]],
                [0, y0, x0, y1],
                [x1, y0, key.size[0], y1],
            ] {
                if x == right || y == bottom {
                    continue;
                }

                let Some(outline) =
                    key.outline.transformed([-(x as f64), -(y as f64)], 1.0)
                else {
                    continue;
                };
                let mut part = key.clone();
                part.outline = outline;
                part.bounds[0] -= x as f64;
                part.bounds[1] -= y as f64;
                part.size = [right - x, bottom - y];
                self.draw_region(
                    (quad, background),
                    transformation,
                    pixels,
                    clip_mask,
                    part,
                    [left + x, top + y],
                );
            }

            if let Some(shader) = background_shader(
                background,
                quad.bounds,
                transformation,
                [0.0; 2],
            ) {
                pixels.fill_rect(
                    tiny_skia::Rect::from_xywh(
                        (left + x0) as f32,
                        (top + y0) as f32,
                        (x1 - x0) as f32,
                        (y1 - y0) as f32,
                    )
                    .expect("nonempty interior"),
                    &tiny_skia::Paint {
                        shader,
                        anti_alias: false,
                        ..Default::default()
                    },
                    tiny_skia::Transform::identity(),
                    clipping_mask(
                        clip_mask,
                        [left + x0, top + y0],
                        [x1 - x0, y1 - y0],
                    ),
                );
            }
        } else {
            self.draw_region(
                (quad, background),
                transformation,
                pixels,
                clip_mask,
                key,
                [left, top],
            );
        }
    }

    fn draw_region(
        &mut self,
        quad: (&Quad, &Background),
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &tiny_skia::Mask,
        key: Key,
        origin: [u32; 2],
    ) {
        let (quad, background) = quad;
        let [left, top] = origin;
        let [width, height] = key.size;
        let masks = self.masks(key);
        let Some(mut fill) = tiny_skia::Pixmap::new(width, height) else {
            return;
        };
        let Some(shader) = background_shader(
            background,
            quad.bounds,
            transformation,
            [left as f32, top as f32],
        ) else {
            return;
        };
        fill.fill_rect(
            tiny_skia::Rect::from_xywh(
                0.0,
                0.0,
                fill.width() as f32,
                fill.height() as f32,
            )
            .expect("valid region"),
            &tiny_skia::Paint {
                shader,
                anti_alias: false,
                ..Default::default()
            },
            tiny_skia::Transform::identity(),
            None,
        );
        let border = into_color(quad.border.color).to_color_u8().premultiply();
        let shadow_color =
            into_color(quad.shadow.color).to_color_u8().premultiply();
        let border =
            [border.red(), border.green(), border.blue(), border.alpha()];
        let shadow_color = [
            shadow_color.red(),
            shadow_color.green(),
            shadow_color.blue(),
            shadow_color.alpha(),
        ];

        for (pixel, [outer, inner, shadow]) in
            fill.data_mut().chunks_exact_mut(4).zip(&masks.coverage)
        {
            if *outer == 255 && *inner == 255 {
                // Filled interior pixels already have their final color.
                continue;
            }

            if *outer == 0 && *shadow == 0 {
                pixel.fill(0);
                continue;
            }

            let band = f32::from(outer.saturating_sub(*inner)) / 255.0;
            let inner = f32::from(*inner) / 255.0;
            let shadow =
                f32::from(*shadow) / 255.0 * (1.0 - f32::from(*outer) / 255.0);

            for channel in 0..4 {
                pixel[channel] = (f32::from(pixel[channel]) * inner
                    + f32::from(border[channel]) * band
                    + f32::from(shadow_color[channel]) * shadow)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }

        pixels.draw_pixmap(
            left as i32,
            top as i32,
            fill.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Nearest,
                ..tiny_skia::PixmapPaint::default()
            },
            tiny_skia::Transform::identity(),
            Some(clip_mask),
        );
    }

    fn masks(&mut self, key: Key) -> Arc<Masks> {
        if let Some(index) =
            self.cache.iter().position(|entry| entry.key == key)
        {
            let entry = self.cache.remove(index).expect("cached mask");
            self.cache.push_front(entry.clone());
            return entry;
        }

        let mut coverage =
            Vec::with_capacity(key.size[0] as usize * key.size[1] as usize);
        let bounds = Outline::new(key.bounds, [0.0; 4], Shape::Circular)
            .expect("valid quad bounds");

        let [left, top, width, height] = key.outline.bounds();
        let limit = width.min(height) * 0.5;
        let extent = key
            .outline
            .radii()
            .into_iter()
            .map(|radius| corner_extent(radius, limit, key.outline.shape()))
            .fold(0.0f64, f64::max);
        let margin = (key.width + 0.5 + key.outline.inset_distance()).max(0.0);
        let mut interior_planes = Vec::with_capacity(12);

        for (index, radius) in key.outline.radii().into_iter().enumerate() {
            if radius == 0.0 {
                continue;
            }

            let curves =
                if key.outline.shape() == Shape::Circular || radius >= limit {
                    crate::core::shape::CIRCULAR_BLEND
                } else {
                    controls(blend(radius, limit))
                };
            let (sx, sy, ox, oy) = match index {
                0 => (1.0, 1.0, left, top),
                1 => (-1.0, 1.0, left + width, top),
                2 => (-1.0, -1.0, left + width, top + height),
                _ => (1.0, -1.0, left, top + height),
            };

            for curve in curves {
                let normal =
                    [curve[0][1] - curve[3][1], curve[3][0] - curve[0][0]];
                let length = normal[0].hypot(normal[1]);
                let normal = normal.map(|v| v / length);
                // A Bézier lies inside the convex hull of its controls. Being
                // inward of every control plane keeps a disc clear of the
                // entire boundary, without approximating edge coverage.
                let offset = curve
                    .into_iter()
                    .map(|p| normal[0] * p[0] + normal[1] * p[1])
                    .fold(f64::NEG_INFINITY, f64::max)
                    * radius;
                let normal = [normal[0] * sx, normal[1] * sy];
                interior_planes.push([
                    normal[0],
                    normal[1],
                    offset + normal[0] * ox + normal[1] * oy,
                ]);
            }
        }

        for y in 0..key.size[1] {
            for x in 0..key.size[0] {
                let point = [x as f64 + 0.5, y as f64 + 0.5];
                let edge = [
                    (point[0] - left).min(left + width - point[0]),
                    (point[1] - top).min(top + height - point[1]),
                ];
                let [bx, by, bw, bh] = key.bounds;
                let clip_edge = (point[0] - bx)
                    .min(bx + bw - point[0])
                    .min((point[1] - by).min(by + bh - point[1]));

                // A disc of radius `margin` stays clear of every corner box
                // and the straight edges. It is wholly inside the reference
                // contour, including the inset and border-width thresholds.
                if edge[0].min(edge[1]) >= margin
                    && clip_edge >= key.width + 0.5
                    && (edge[0].max(edge[1]) >= extent + margin
                        || interior_planes.iter().all(|[nx, ny, d]| {
                            nx * point[0] + ny * point[1] - d >= margin
                        }))
                {
                    coverage.push([255, 255, 0]);
                    continue;
                }

                let distance = bounds
                    .signed_distance(point)
                    .max(key.outline.signed_distance(point));
                let outer = edge_coverage(distance);
                let inner = edge_coverage(distance + key.width);
                let shadow = if key.shadow && outer < 1.0 {
                    let point = [
                        point[0] - key.shadow_offset[0],
                        point[1] - key.shadow_offset[1],
                    ];
                    let distance = bounds
                        .signed_distance(point)
                        .max(key.outline.signed_distance(point));

                    if key.blur > 0.0 {
                        let t = ((distance.max(0.0) + key.blur)
                            / (2.0 * key.blur))
                            .clamp(0.0, 1.0);
                        1.0 - t * t * (3.0 - 2.0 * t)
                    } else {
                        edge_coverage(distance)
                    }
                } else {
                    0.0
                };
                coverage.push(
                    [outer, inner, shadow].map(|v| (v * 255.0).round() as u8),
                );
            }
        }

        let entry = Arc::new(Masks { key, coverage });
        let bytes = entry.coverage.len() * 3;

        if bytes <= CACHE_BYTES {
            while self.bytes + bytes > CACHE_BYTES
                || self.cache.len() >= CACHE_ENTRIES
            {
                let removed = self.cache.pop_back().expect("mask cache budget");
                self.bytes -= removed.coverage.len() * 3;
            }

            self.bytes += bytes;
            self.cache.push_front(entry.clone());
        }

        entry
    }
}

fn interior(key: &Key) -> Option<[u32; 4]> {
    let [x0, y0, x1, y1] =
        full_coverage_bounds(key.outline, key.bounds, key.size, key.width)?;
    (u64::from(x1 - x0) * u64::from(y1 - y0) >= 4096)
        .then_some([x0, y0, x1, y1])
}

pub(crate) fn full_coverage_bounds(
    outline: Outline,
    bounds: [f64; 4],
    size: [u32; 2],
    border: f64,
) -> Option<[u32; 4]> {
    let [left, top, width, height] = outline.bounds();
    let limit = width.min(height) * 0.5;
    let extent = outline
        .radii()
        .into_iter()
        .map(|radius| corner_extent(radius, limit, outline.shape()))
        .fold(0.0f64, f64::max);
    let margin = (border + 0.5 + outline.inset_distance()).max(0.0);
    let [bx, by, bw, bh] = bounds;
    let x0 = (left + extent + margin)
        .max(bx + border + 0.5)
        .ceil()
        .max(0.0)
        .min(size[0] as f64) as u32;
    let y0 = (top + extent + margin)
        .max(by + border + 0.5)
        .ceil()
        .max(0.0)
        .min(size[1] as f64) as u32;
    let x1 = (left + width - extent - margin)
        .min(bx + bw - border - 0.5)
        .floor()
        .max(0.0)
        .min(size[0] as f64) as u32;
    let y1 = (top + height - extent - margin)
        .min(by + bh - border - 0.5)
        .floor()
        .max(0.0)
        .min(size[1] as f64) as u32;

    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

fn background_shader(
    background: &Background,
    bounds: Rectangle,
    transformation: Transformation,
    origin: [f32; 2],
) -> Option<tiny_skia::Shader<'static>> {
    match background {
        Background::Color(color) => {
            Some(tiny_skia::Shader::SolidColor(into_color(*color)))
        }
        Background::Gradient(Gradient::Linear(linear)) => {
            let (start, end) = linear.angle.to_distance(&bounds);
            let mut stops = linear
                .stops
                .into_iter()
                .flatten()
                .map(|stop| {
                    tiny_skia::GradientStop::new(
                        stop.offset,
                        into_color(stop.color),
                    )
                })
                .collect::<Vec<_>>();
            if stops.is_empty() {
                stops.push(tiny_skia::GradientStop::new(
                    0.0,
                    tiny_skia::Color::BLACK,
                ));
            }
            let scale = transformation.scale_factor();
            let translation = transformation.translation();
            tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(start.x, start.y),
                tiny_skia::Point::from_xy(end.x, end.y),
                stops,
                tiny_skia::SpreadMode::Pad,
                tiny_skia::Transform::from_row(
                    scale,
                    0.0,
                    0.0,
                    scale,
                    translation.x - origin[0],
                    translation.y - origin[1],
                ),
            )
        }
    }
}

fn clipping_mask(
    mask: &tiny_skia::Mask,
    origin: [u32; 2],
    size: [u32; 2],
) -> Option<&tiny_skia::Mask> {
    let [left, top] = origin;
    let [width, height] = size;
    let Some(right) = left.checked_add(width) else {
        return Some(mask);
    };
    let Some(bottom) = top.checked_add(height) else {
        return Some(mask);
    };
    if right > mask.width() || bottom > mask.height() {
        return Some(mask);
    }
    let full = mask
        .data()
        .chunks_exact(mask.width() as usize)
        .skip(top as usize)
        .take(height as usize)
        .all(|row| {
            let mut chunks =
                row[left as usize..right as usize].chunks_exact(64);
            chunks.all(|chunk| chunk == [255; 64])
                && chunks.remainder().iter().all(|alpha| *alpha == 255)
        });
    (!full).then_some(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        Key {
            outline: Outline::new(
                [1.25, 2.5, 56.0, 44.0],
                [8.25; 4],
                Shape::Continuous,
            )
            .unwrap(),
            bounds: [1.25, 2.5, 56.0, 44.0],
            size: [60, 50],
            width: 0.75,
            shadow_offset: [2.25, 3.5],
            blur: 3.0,
            shadow: true,
        }
    }

    #[test]
    fn stable_contours_reuse_masks_and_profile_changes_invalidate_them() {
        let mut pipeline = Pipeline::default();
        let key = key();
        let first = pipeline.masks(key.clone());
        let second = pipeline.masks(key.clone());
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(pipeline.bytes, 60 * 50 * 3);
        let circular = Key {
            outline: Outline::new(
                key.outline.bounds(),
                key.outline.radii(),
                Shape::Circular,
            )
            .unwrap(),
            ..key
        };
        let third = pipeline.masks(circular);
        assert!(!Arc::ptr_eq(&first, &third));
        assert_ne!(first.coverage, third.coverage);
    }

    #[test]
    fn inset_is_measured_from_the_original_contour_and_can_remove_the_fill() {
        let mut pipeline = Pipeline::default();
        let key = key();
        let inset = key.outline.inset(30.0).unwrap();
        let masks = pipeline.masks(Key {
            outline: inset,
            ..key
        });
        assert!(
            masks
                .coverage
                .iter()
                .all(|sample| sample[0] == 0 && sample[1] == 0)
        );
    }

    #[test]
    fn interior_shortcuts_match_full_distance_coverage() {
        let mut pipeline = Pipeline::default();

        for shape in [Shape::Circular, Shape::Continuous] {
            for radii in [[0.0; 4], [8.25; 4], [0.0, 12.0, 3.5, 9.0], [22.0; 4]]
            {
                for inset in [-3.0, 0.0, 2.25, 30.0] {
                    for width in [0.0, 0.75, 3.0, 24.0] {
                        let key = Key {
                            outline: Outline::new(
                                [1.25, 2.5, 56.0, 44.0],
                                radii,
                                shape,
                            )
                            .unwrap()
                            .inset(inset)
                            .unwrap(),
                            width,
                            ..key()
                        };
                        let bounds =
                            Outline::new(key.bounds, [0.0; 4], Shape::Circular)
                                .unwrap();
                        let masks = pipeline.masks(key.clone());

                        for y in 0..key.size[1] {
                            for x in 0..key.size[0] {
                                let point = [x as f64 + 0.5, y as f64 + 0.5];
                                let distance = bounds
                                    .signed_distance(point)
                                    .max(key.outline.signed_distance(point));
                                let expected = [
                                    edge_coverage(distance),
                                    edge_coverage(distance + width),
                                ]
                                .map(|value| (value * 255.0).round() as u8);
                                let actual = masks.coverage
                                    [(y * key.size[0] + x) as usize];
                                assert_eq!(
                                    [actual[0], actual[1]],
                                    expected,
                                    "shape={shape:?} radii={radii:?} inset={inset} width={width} point={point:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cache_has_an_entry_limit() {
        let mut pipeline = Pipeline::default();

        for width in 0..CACHE_ENTRIES * 2 {
            let _ = pipeline.masks(Key {
                width: width as f64,
                ..key()
            });
        }

        assert_eq!(pipeline.cache.len(), CACHE_ENTRIES);
        assert!(pipeline.bytes <= CACHE_BYTES);
    }

    #[test]
    fn tiled_interiors_match_full_surface_compositing() {
        use crate::core::{Border, Color, Shadow, Vector, gradient};
        let backgrounds = [
            Background::Color(Color::from_rgba(0.2, 0.4, 0.8, 1.0)),
            Background::Color(Color::from_rgba(0.2, 0.4, 0.8, 0.35)),
            Background::Color(Color::TRANSPARENT),
            Background::Gradient(
                gradient::Linear::new(0.7)
                    .add_stop(0.0, Color::from_rgba(0.8, 0.3, 0.1, 0.25))
                    .add_stop(1.0, Color::from_rgba(0.1, 0.4, 0.7, 0.9))
                    .into(),
            ),
        ];
        for shape in [Shape::Circular, Shape::Continuous] {
            for scale in [1.0, 1.25, 2.0] {
                for inset in [-3.0, 0.0, 5.25] {
                    for (width, shadow) in
                        [(0.0, false), (0.75, true), (3.0, false)]
                    {
                        for background in backgrounds {
                            let bounds = Rectangle {
                                x: 12.25,
                                y: 24.5,
                                width: 270.0,
                                height: 190.0,
                            };
                            let outline = Outline::new(
                                [
                                    bounds.x,
                                    bounds.y,
                                    bounds.width,
                                    bounds.height,
                                ]
                                .map(f64::from),
                                [0.0, 24.0, 8.5, 12.0],
                                shape,
                            )
                            .unwrap()
                            .inset(inset)
                            .unwrap();
                            let quad = Quad {
                                bounds,
                                border: Border {
                                    width,
                                    color: Color::from_rgba(0.8, 0.1, 0.3, 0.6),
                                    outline: Some(outline),
                                    shape,
                                    ..Border::default()
                                },
                                shadow: Shadow {
                                    color: if shadow {
                                        Color::BLACK.scale_alpha(0.4)
                                    } else {
                                        Color::TRANSPARENT
                                    },
                                    offset: Vector::new(3.25, -5.5),
                                    blur_radius: 6.0,
                                },
                                snap: false,
                                ..Quad::default()
                            };
                            let transformation =
                                Transformation::translate(2.5, 1.75)
                                    * Transformation::scale(scale);
                            let mut actual =
                                tiny_skia::Pixmap::new(420, 300).unwrap();
                            actual.fill(
                                tiny_skia::Color::from_rgba(0.1, 0.5, 0.3, 0.7)
                                    .unwrap(),
                            );
                            let mut expected = actual.clone();
                            let mut clip =
                                tiny_skia::Mask::new(420, 300).unwrap();
                            for (index, alpha) in
                                clip.data_mut().iter_mut().enumerate()
                            {
                                *alpha = if inset != 0.0 && index % 7 == 0 {
                                    128
                                } else {
                                    255
                                };
                            }
                            Pipeline::default().draw(
                                &quad,
                                &background,
                                transformation,
                                &mut actual.as_mut(),
                                &clip,
                                Rectangle {
                                    x: 0.0,
                                    y: 0.0,
                                    width: 420.0,
                                    height: 300.0,
                                },
                            );
                            let bounds = bounds * transformation;
                            let outline = outline
                                .transformed([2.5, 1.75], scale as f64)
                                .unwrap();
                            Pipeline::default().draw_region(
                                (&quad, &background),
                                transformation,
                                &mut expected.as_mut(),
                                &clip,
                                Key {
                                    outline,
                                    bounds: [
                                        bounds.x,
                                        bounds.y,
                                        bounds.width,
                                        bounds.height,
                                    ]
                                    .map(f64::from),
                                    size: [420, 300],
                                    width: f64::from(width * scale),
                                    shadow_offset: [
                                        3.25 * scale as f64,
                                        -5.5 * scale as f64,
                                    ],
                                    blur: 6.0 * scale as f64,
                                    shadow,
                                },
                                [0, 0],
                            );
                            let delta = actual
                                .data()
                                .iter()
                                .zip(expected.data())
                                .map(|(a, b)| a.abs_diff(*b))
                                .max()
                                .unwrap();
                            assert!(
                                delta <= 1,
                                "delta={delta} shape={shape:?} scale={scale} inset={inset} width={width} shadow={shadow} background={background:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn large_panels_cache_only_the_perimeter() {
        use crate::core::{Border, Color};
        let quad = Quad {
            bounds: Rectangle {
                x: 0.0,
                y: 0.0,
                width: 3840.0,
                height: 2160.0,
            },
            border: Border {
                radius: 12.0.into(),
                shape: Shape::Continuous,
                ..Border::default()
            },
            ..Quad::default()
        };
        let mut pixels = tiny_skia::Pixmap::new(3840, 2160).unwrap();
        let mut clip = tiny_skia::Mask::new(3840, 2160).unwrap();
        clip.data_mut().fill(255);
        let mut pipeline = Pipeline::default();
        pipeline.draw(
            &quad,
            &Background::Color(Color::WHITE),
            Transformation::IDENTITY,
            &mut pixels.as_mut(),
            &clip,
            quad.bounds,
        );
        assert_eq!(pipeline.cache.len(), 4);
        assert!(
            pipeline.bytes < 1024 * 1024,
            "cached {} bytes",
            pipeline.bytes
        );
        assert!(pixels.pixel(1920, 1080).unwrap().alpha() == 255);
    }
}
