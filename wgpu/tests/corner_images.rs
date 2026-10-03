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
    });
}

#[test]
fn software_images_use_destination_scale_contours() {
    let mut renderer =
        iced_tiny_skia::Renderer::new(Font::default(), Pixels(16.0));
    check_images(&mut renderer);
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
