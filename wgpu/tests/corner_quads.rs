use iced_wgpu::core::border::{Border, Outline, Radius, Shape};
use iced_wgpu::core::renderer::{Headless, Quad};
use iced_wgpu::core::shape::edge_coverage;
use iced_wgpu::core::{
    Background, Color, Font, Pixels, Rectangle, Renderer, Shadow, Size,
    Transformation, Vector,
};

#[test]
#[ignore = "requires a GPU or software Vulkan adapter"]
fn fills_borders_shadows_and_reference_insets_match_distance_coverage() {
    futures::executor::block_on(async {
        let mut renderer = <iced_wgpu::Renderer as Headless>::new(
            Font::default(),
            Pixels(16.0),
            None,
        )
        .await
        .expect("GPU renderer");
        check_quads(&mut renderer);
    });
}

#[test]
fn software_quads_match_distance_coverage() {
    let mut renderer =
        iced_tiny_skia::Renderer::new(Font::default(), Pixels(16.0));
    check_quads(&mut renderer);
}

fn check_quads(renderer: &mut (impl Renderer + Headless)) {
    for scale in [1.0, 1.25, 1.5] {
        for local_scale in [1.0f32, 1.1] {
            let translation = Vector::new(3.25, 2.75);
            let transformation =
                Transformation::translate(translation.x, translation.y)
                    * Transformation::scale(local_scale);
            for radii in [
                [0.0; 4],
                [0.375; 4],
                [8.25; 4],
                [22.0; 4],
                [0.0, 16.0, 5.0, 10.0],
            ] {
                for width in [0.0, 0.75, 3.5, 30.0] {
                    for (inset, gradient, use_reference, snap) in [
                        (0.0, false, false, false),
                        (0.0, false, false, true),
                        (4.0, false, true, false),
                        (4.0, false, true, true),
                        (24.0, false, true, false),
                        (4.0, true, true, false),
                        (0.0, true, false, true),
                    ] {
                        renderer
                            .reset(Rectangle::with_size(Size::new(90.0, 80.0)));
                        let bounds = Rectangle {
                            x: 13.25,
                            y: 12.5,
                            width: 56.0,
                            height: 44.0,
                        };
                        let parent = Outline::new(
                            [13.25, 12.5, 56.0, 44.0],
                            radii.map(f64::from),
                            Shape::Continuous,
                        )
                        .unwrap();
                        let mut border = Border {
                            radius: Radius::from(radii),
                            width,
                            color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                            ..Default::default()
                        }
                        .shape(Shape::Continuous);

                        if use_reference {
                            border = border
                                .rounded(0.0)
                                .outline(parent.inset(inset).unwrap());
                        }
                        let quad = Quad {
                            bounds,
                            border,
                            shadow: Shadow {
                                color: Color::from_rgba(1.0, 1.0, 1.0, 0.4),
                                offset: Vector::new(2.25, 3.5),
                                blur_radius: 3.0,
                            },
                            snap,
                        };
                        let fill = Color::from_rgba(1.0, 1.0, 1.0, 0.65);
                        let background = if gradient {
                            Background::Gradient(
                                iced_wgpu::core::gradient::Linear::new(0.7)
                                    .add_stop(0.0, fill)
                                    .add_stop(1.0, fill)
                                    .into(),
                            )
                        } else {
                            Background::Color(fill)
                        };
                        renderer.with_transformation(
                            transformation,
                            |renderer| {
                                renderer.fill_quad(quad, background);
                            },
                        );
                        let size = Size::new(
                            (90.0 * scale) as u32,
                            (80.0 * scale) as u32,
                        );
                        let bytes = Headless::screenshot(
                            renderer,
                            size,
                            scale,
                            Color::TRANSPARENT,
                        );
                        let mut reference = parent
                            .inset(inset)
                            .unwrap()
                            .transformed(
                                [translation.x as f64, translation.y as f64],
                                local_scale as f64,
                            )
                            .unwrap()
                            .transformed([0.0; 2], f64::from(scale))
                            .unwrap();
                        let own_bounds = quad.bounds * transformation;
                        let mut physical_bounds = [
                            own_bounds.x,
                            own_bounds.y,
                            own_bounds.width,
                            own_bounds.height,
                        ]
                        .map(|v| f64::from(v * scale));

                        if snap {
                            let right = (physical_bounds[0]
                                + physical_bounds[2]
                                + 0.001)
                                .round();
                            let bottom = (physical_bounds[1]
                                + physical_bounds[3]
                                + 0.001)
                                .round();
                            physical_bounds[0] =
                                (physical_bounds[0] + 0.001).round();
                            physical_bounds[1] =
                                (physical_bounds[1] + 0.001).round();
                            physical_bounds[2] = right - physical_bounds[0];
                            physical_bounds[3] = bottom - physical_bounds[1];

                            if !use_reference {
                                reference = Outline::new(
                                    physical_bounds,
                                    reference.radii(),
                                    Shape::Continuous,
                                )
                                .unwrap();
                            }
                        }

                        let bounds = Outline::new(
                            physical_bounds,
                            [0.0; 4],
                            Shape::Circular,
                        )
                        .unwrap();
                        for y in 0..size.height {
                            for x in 0..size.width {
                                let p = [x as f64 + 0.5, y as f64 + 0.5];
                                let distance = bounds
                                    .signed_distance(p)
                                    .max(reference.signed_distance(p));
                                let outer = edge_coverage(distance);
                                let inner = edge_coverage(
                                    distance
                                        + f64::from(
                                            width * local_scale * scale,
                                        ),
                                );
                                let q = [
                                    p[0] - f64::from(
                                        2.25 * local_scale * scale,
                                    ),
                                    p[1] - f64::from(3.5 * local_scale * scale),
                                ];
                                let shadow_distance = bounds
                                    .signed_distance(q)
                                    .max(reference.signed_distance(q));
                                let blur = f64::from(3.0 * local_scale * scale);
                                let t = ((shadow_distance.max(0.0) + blur)
                                    / (2.0 * blur))
                                    .clamp(0.0, 1.0);
                                let shadow = 1.0 - t * t * (3.0 - 2.0 * t);
                                let expected = 0.65 * inner
                                    + 0.9 * (outer - inner)
                                    + 0.4 * shadow * (1.0 - outer);
                                let actual = bytes
                                    [((y * size.width + x) * 4 + 3) as usize]
                                    as f64
                                    / 255.0;
                                assert!(
                                    (actual - expected).abs() <= 2.0 / 255.0,
                                    "scale={scale} radii={radii:?} width={width} inset={inset} p={p:?}: alpha={actual}, reference={expected}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn software_quad_regions_respect_hard_clipping() {
    let mut renderer =
        iced_tiny_skia::Renderer::new(Font::default(), Pixels(16.0));
    let viewport = Rectangle::with_size(Size::new(90.0, 80.0));
    let clip = Rectangle {
        x: 20.0,
        y: 16.0,
        width: 40.0,
        height: 44.0,
    };
    let parent =
        Outline::new([13.25, 12.5, 56.0, 44.0], [12.0; 4], Shape::Continuous)
            .unwrap();

    for scale in [1.0f32, 1.25, 1.5] {
        for inset in [0.0, 3.25, 24.0] {
            let quad = Quad {
                bounds: Rectangle {
                    x: 13.25,
                    y: 12.5,
                    width: 56.0,
                    height: 44.0,
                },
                border: Border {
                    color: Color::from_rgba(0.7, 0.2, 0.1, 0.6),
                    width: 1.25,
                    ..Default::default()
                }
                .outline(parent.inset(inset).unwrap()),
                shadow: Shadow {
                    color: Color::from_rgba(0.1, 0.2, 0.7, 0.5),
                    offset: Vector::new(2.25, 3.5),
                    blur_radius: 4.0,
                },
                ..Default::default()
            };
            let background = Background::Gradient(
                iced_wgpu::core::gradient::Linear::new(0.7)
                    .add_stop(0.0, Color::from_rgba(0.2, 0.7, 0.3, 0.5))
                    .add_stop(1.0, Color::from_rgba(0.8, 0.4, 0.1, 0.9))
                    .into(),
            );
            let size = Size::new((90.0 * scale) as u32, (80.0 * scale) as u32);
            renderer.reset(viewport);
            renderer.fill_quad(quad, background);
            let full = Headless::screenshot(
                &mut renderer,
                size,
                scale,
                Color::TRANSPARENT,
            );
            renderer.reset(viewport);
            renderer.with_layer(clip, |renderer| {
                renderer.fill_quad(quad, background)
            });
            let clipped = Headless::screenshot(
                &mut renderer,
                size,
                scale,
                Color::TRANSPARENT,
            );

            for y in 0..size.height {
                for x in 0..size.width {
                    let index = ((y * size.width + x) * 4) as usize;
                    let p = iced_wgpu::core::Point::new(
                        (x as f32 + 0.5) / scale,
                        (y as f32 + 0.5) / scale,
                    );
                    let expected = if clip.contains(p) {
                        &full[index..index + 4]
                    } else {
                        &[0; 4]
                    };
                    assert_eq!(
                        &clipped[index..index + 4],
                        expected,
                        "scale={scale} inset={inset} p={p:?}"
                    );
                }
            }
        }
    }
}
