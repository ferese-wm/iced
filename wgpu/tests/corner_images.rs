#![cfg(feature = "image")]
use iced_wgpu::core::border::{Outline, Radius, Shape};
use iced_wgpu::core::image::{self, FilterMethod, Handle};
use iced_wgpu::core::renderer::Headless;
use iced_wgpu::core::shape::edge_coverage;
use iced_wgpu::core::{Color, Font, Pixels, Rectangle, Renderer, Size};

#[test]
#[ignore = "requires a GPU or software Vulkan adapter"]
fn gpu_images_use_destination_scale_contours() {
    futures::executor::block_on(async {
        let mut renderer = <iced_wgpu::Renderer as Headless>::new(
            Font::default(),
            Pixels(16.0),
            None,
        )
        .await
        .unwrap();
        check_images(&mut renderer);
        check_invalid_image_bounds(&mut renderer);
    });
}

#[test]
fn software_images_use_destination_scale_contours() {
    let mut renderer =
        iced_tiny_skia::Renderer::new(Font::default(), Pixels(16.0));
    check_images(&mut renderer);
    check_invalid_image_bounds(&mut renderer);
}

fn check_images(
    renderer: &mut (impl Headless + image::Renderer<Handle = Handle>),
) {
    let mut rgba = vec![0; 20 * 12 * 4];

    for y in 0..12 {
        for x in 0..20 {
            let color = match (x < 10, y < 6) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [0, 0, 255, 255],
                (false, false) => [255, 255, 255, 255],
            };
            rgba[(y * 20 + x) * 4..(y * 20 + x + 1) * 4]
                .copy_from_slice(&color);
        }
    }

    let handle = Handle::from_rgba(20, 12, rgba);

    for scale in [1.0f32, 1.25, 1.5] {
        for shape in [Shape::Circular, Shape::Continuous] {
            for radii in [[0.0f32; 4], [8.25; 4], [0.0, 12.0, 3.5, 9.0]] {
                for (rotation, filter, snap) in [
                    (0.0f32, FilterMethod::Nearest, false),
                    (0.0f32, FilterMethod::Nearest, true),
                    (0.37, FilterMethod::Nearest, false),
                    (-0.61, FilterMethod::Nearest, true),
                    (0.37, FilterMethod::Linear, false),
                ] {
                    for inset in [0.0f64, 3.25, 18.0] {
                        renderer
                            .reset(Rectangle::with_size(Size::new(90.0, 80.0)));
                        let mut bounds = Rectangle {
                            x: 12.25,
                            y: 9.5,
                            width: 56.5,
                            height: 37.25,
                        };
                        let mut clip = Rectangle {
                            x: 17.5,
                            y: 15.25,
                            width: 40.25,
                            height: 28.75,
                        };
                        let parent = Outline::new(
                            [17.5, 15.25, 40.25, 28.75],
                            radii.map(f64::from),
                            shape,
                        )
                        .unwrap();
                        let mut image = image::Image::new(handle.clone())
                            .filter_method(filter)
                            .rotation(rotation)
                            .shape(shape)
                            .snap(snap);
                        image.border_radius = Radius::from(radii);

                        if inset != 0.0 {
                            image = image.outline(parent.inset(inset).unwrap());
                            image.border_radius = Radius::default();
                        }

                        renderer.draw_image(image, bounds, clip);
                        let size = Size::new(
                            (90.0 * scale) as u32,
                            (80.0 * scale) as u32,
                        );
                        let pixels = Headless::screenshot(
                            renderer,
                            size,
                            scale,
                            Color::TRANSPARENT,
                        );
                        if snap {
                            let snap_bounds = |bounds: Rectangle| {
                                let x = (bounds.x * scale + 0.001).round();
                                let y = (bounds.y * scale + 0.001).round();
                                let right = ((bounds.x + bounds.width) * scale
                                    + 0.001)
                                    .round();
                                let bottom = ((bounds.y + bounds.height)
                                    * scale
                                    + 0.001)
                                    .round();
                                Rectangle {
                                    x: x / scale,
                                    y: y / scale,
                                    width: (right - x) / scale,
                                    height: (bottom - y) / scale,
                                }
                            };
                            bounds = snap_bounds(bounds);
                            clip = snap_bounds(clip);
                        }

                        let reference_parent = if inset == 0.0 {
                            Outline::new(
                                [clip.x, clip.y, clip.width, clip.height]
                                    .map(f64::from),
                                radii.map(f64::from),
                                shape,
                            )
                            .unwrap()
                        } else {
                            parent
                        };
                        let outline = reference_parent
                            .inset(inset)
                            .unwrap()
                            .transformed([0.0; 2], scale as f64)
                            .unwrap();
                        let clip_outline = Outline::new(
                            [clip.x, clip.y, clip.width, clip.height]
                                .map(f64::from),
                            [0.0; 4],
                            Shape::Circular,
                        )
                        .unwrap()
                        .transformed([0.0; 2], scale as f64)
                        .unwrap();
                        let center = [
                            (bounds.x + bounds.width / 2.0) as f64,
                            (bounds.y + bounds.height / 2.0) as f64,
                        ];
                        let (sin, cos) = (rotation as f64).sin_cos();

                        for y in 0..size.height {
                            for x in 0..size.width {
                                let p = [x as f64 + 0.5, y as f64 + 0.5];
                                let delta = [
                                    p[0] / scale as f64 - center[0],
                                    p[1] / scale as f64 - center[1],
                                ];
                                let source = [
                                    delta[0] * cos - delta[1] * sin + center[0],
                                    delta[0] * sin + delta[1] * cos + center[1],
                                ];
                                let uv = [
                                    (source[0] - bounds.x as f64)
                                        / bounds.width as f64,
                                    (source[1] - bounds.y as f64)
                                        / bounds.height as f64,
                                ];
                                let source_outline = Outline::new(
                                    [
                                        bounds.x as f64,
                                        bounds.y as f64,
                                        bounds.width as f64,
                                        bounds.height as f64,
                                    ],
                                    [0.0; 4],
                                    Shape::Circular,
                                )
                                .unwrap();
                                let image_distance = source_outline
                                    .signed_distance(source)
                                    * scale as f64;
                                let coverage = edge_coverage(
                                    outline
                                        .signed_distance(p)
                                        .max(clip_outline.signed_distance(p))
                                        .max(image_distance),
                                );
                                let expected = coverage;
                                let index = ((y * size.width + x) * 4) as usize;
                                let actual = pixels[index + 3] as f64 / 255.0;
                                assert!(
                                    (actual - expected).abs() <= 2.0 / 255.0,
                                    "shape={shape:?} scale={scale} radii={radii:?} rotation={rotation} inset={inset} p={p:?}: alpha={actual}, reference={expected}"
                                );

                                if expected == 1.0
                                    && (uv[0] - 0.5).abs() > 0.06
                                    && (uv[1] - 0.5).abs() > 0.1
                                {
                                    let expected =
                                        match (uv[0] < 0.5, uv[1] < 0.5) {
                                            (true, true) => [255, 0, 0],
                                            (false, true) => [0, 255, 0],
                                            (true, false) => [0, 0, 255],
                                            (false, false) => [255; 3],
                                        };
                                    assert_eq!(
                                        &pixels[index..index + 3],
                                        &expected,
                                        "rotated texel at {p:?} uv={uv:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "svg")]
#[test]
#[ignore = "requires a GPU or software Vulkan adapter"]
fn svg_rotation_agrees_between_backends() {
    use iced_wgpu::core::svg::{self, Renderer as _};

    futures::executor::block_on(async {
        let mut gpu = <iced_wgpu::Renderer as Headless>::new(
            Font::default(),
            Pixels(16.0),
            None,
        )
        .await
        .unwrap();
        let mut cpu =
            iced_tiny_skia::Renderer::new(Font::default(), Pixels(16.0));
        let handle = svg::Handle::from_memory(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><path fill="red" d="M0 0H40V20H0Z"/><path fill="lime" d="M40 0H80V20H40Z"/><path fill="blue" d="M0 20H40V40H0Z"/><path fill="white" d="M40 20H80V40H40Z"/></svg>"#.as_slice()
        );

        for scale in [1.0, 1.25, 1.5] {
            for rotation in [-0.61f32, 0.0, 0.37] {
                let viewport = Rectangle::with_size(Size::new(120.0, 100.0));
                let bounds = Rectangle {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 40.0,
                };
                let clip = Rectangle {
                    x: 32.0,
                    y: 20.0,
                    width: 70.0,
                    height: 65.0,
                };
                let image = svg::Svg::new(handle.clone()).rotation(rotation);
                gpu.reset(viewport);
                cpu.reset(viewport);
                gpu.draw_svg(image.clone(), bounds, clip);
                cpu.draw_svg(image, bounds, clip);
                let size =
                    Size::new((120.0 * scale) as u32, (100.0 * scale) as u32);
                let gp = Headless::screenshot(
                    &mut gpu,
                    size,
                    scale,
                    Color::TRANSPARENT,
                );
                let cp = Headless::screenshot(
                    &mut cpu,
                    size,
                    scale,
                    Color::TRANSPARENT,
                );
                let (sin, cos) = rotation.sin_cos();
                let center = bounds.center();
                let mut checked = 0;

                for y in 0..size.height {
                    for x in 0..size.width {
                        let p = [
                            (x as f32 + 0.5) / scale,
                            (y as f32 + 0.5) / scale,
                        ];
                        let dx = p[0] - center.x;
                        let dy = p[1] - center.y;
                        let source = [
                            dx * cos - dy * sin + center.x,
                            dx * sin + dy * cos + center.y,
                        ];
                        // Compare interior texels independently of the edge filters.
                        if p[0] < clip.x + 2.0
                            || p[0] > clip.x + clip.width - 2.0
                            || p[1] < clip.y + 2.0
                            || p[1] > clip.y + clip.height - 2.0
                            || source[0] < bounds.x + 2.0
                            || source[0] > bounds.x + bounds.width - 2.0
                            || source[1] < bounds.y + 2.0
                            || source[1] > bounds.y + bounds.height - 2.0
                            || (source[0] - center.x).abs() < 2.0
                            || (source[1] - center.y).abs() < 2.0
                        {
                            continue;
                        }

                        let expected = match (
                            source[0] < center.x,
                            source[1] < center.y,
                        ) {
                            (true, true) => [255, 0, 0, 255],
                            (false, true) => [0, 255, 0, 255],
                            (true, false) => [0, 0, 255, 255],
                            (false, false) => [255; 4],
                        };
                        let index = ((y * size.width + x) * 4) as usize;
                        assert_eq!(
                            &gp[index..index + 4],
                            &expected,
                            "GPU rotation={rotation} p={p:?}"
                        );
                        assert_eq!(
                            &cp[index..index + 4],
                            &expected,
                            "software rotation={rotation} p={p:?}"
                        );
                        checked += 1;
                    }
                }

                assert!(checked > 500);
            }
        }
    });
}

fn check_invalid_image_bounds(
    renderer: &mut (impl Headless + image::Renderer<Handle = Handle>),
) {
    let handle = Handle::from_rgba(1, 1, vec![255; 4]);
    let viewport = Rectangle::with_size(Size::new(40.0, 40.0));
    let reference =
        Outline::new([0.0, 0.0, 40.0, 40.0], [8.0; 4], Shape::Continuous)
            .unwrap();

    for outline in [None, Some(reference)] {
        for width in [0.0f32, -1.0, 0.25, f32::NAN, f32::INFINITY] {
            renderer.reset(viewport);
            let mut image = image::Image::new(handle.clone())
                .shape(Shape::Continuous)
                .snap(true);
            image.outline = outline;
            renderer.draw_image(
                image,
                Rectangle {
                    x: 10.0,
                    y: 10.0,
                    width,
                    height: 20.0,
                },
                viewport,
            );
            let pixels = Headless::screenshot(
                renderer,
                Size::new(40, 40),
                1.0,
                Color::TRANSPARENT,
            );
            assert!(
                pixels.iter().all(|p| *p == 0),
                "invalid width={width:?} outline={outline:?}"
            );
        }
    }
}
