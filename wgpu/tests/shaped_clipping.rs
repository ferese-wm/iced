use iced_wgpu::core::border::{Outline, Shape};
use iced_wgpu::core::renderer::{Headless, Quad};
use iced_wgpu::core::shape::edge_coverage;
use iced_wgpu::core::{Color, Font, Pixels, Rectangle, Renderer, Size};

#[test]
#[ignore = "requires a GPU or software Vulkan adapter"]
fn gpu_masks_entire_groups_and_preserves_painter_order() {
    futures::executor::block_on(async {
        let mut renderer = <iced_wgpu::Renderer as Headless>::new(
            Font::default(),
            Pixels(16.0),
            None,
        )
        .await
        .unwrap();
        check(&mut renderer);
        check_insets_and_shape_changes(&mut renderer);
        check_border_clips_follow_snapped_background(&mut renderer);
        #[cfg(all(feature = "image", feature = "geometry"))]
        check_child_primitives(&mut renderer);
    });
}

#[test]
fn software_masks_entire_groups_and_preserves_painter_order() {
    let mut renderer =
        iced_tiny_skia::Renderer::new(Font::default(), Pixels(16.0));
    check(&mut renderer);
    check_insets_and_shape_changes(&mut renderer);
    check_border_clips_follow_snapped_background(&mut renderer);
    #[cfg(all(feature = "image", feature = "geometry"))]
    check_child_primitives(&mut renderer);
}

fn check(renderer: &mut (impl Renderer + Headless)) {
    let screen = Rectangle::with_size(Size::new(90.0, 80.0));
    let bounds = Rectangle {
        x: 13.25,
        y: 12.5,
        width: 56.0,
        height: 44.0,
    };
    let parent =
        Outline::new([13.25, 12.5, 56.0, 44.0], [12.0; 4], Shape::Continuous)
            .unwrap();

    for scale in [1.0f32, 1.25, 1.5] {
        for nested in [0, 1, 3, 17] {
            renderer.reset(screen);
            renderer.with_layer(screen, |renderer| {
                renderer.with_shaped_layer(bounds, parent, |renderer| {
                    renderer.fill_quad(
                        Quad {
                            bounds: screen,
                            snap: false,
                            ..Default::default()
                        },
                        Color::from_rgba(1.0, 1.0, 1.0, 0.4),
                    );

                    for _ in 0..nested {
                        renderer.start_shaped_layer(bounds, parent);
                    }

                    renderer.fill_quad(
                        Quad {
                            bounds: screen,
                            snap: false,
                            ..Default::default()
                        },
                        Color::from_rgba(1.0, 1.0, 1.0, 0.7),
                    );

                    for _ in 0..nested {
                        renderer.end_layer();
                    }
                });
            });
            let size = Size::new((90.0 * scale) as u32, (80.0 * scale) as u32);
            let bytes =
                Headless::screenshot(renderer, size, scale, Color::TRANSPARENT);
            let outline = parent.transformed([0.0; 2], scale as f64).unwrap();

            for y in 0..size.height {
                for x in 0..size.width {
                    let coverage = edge_coverage(
                        outline
                            .signed_distance([x as f64 + 0.5, y as f64 + 0.5]),
                    );
                    let expected = (0.7 + 0.4 * 0.3) * coverage;
                    let actual = bytes[((y * size.width + x) * 4 + 3) as usize]
                        as f64
                        / 255.0;
                    assert!(
                        (actual - expected).abs() <= 2.0 / 255.0,
                        "nested={nested} scale={scale} at ({x},{y}): {actual} vs {expected}"
                    );
                }
            }

            // Content recorded after the enclosing rectangular layer must stay on top.
            renderer.fill_quad(
                Quad {
                    bounds: screen,
                    snap: false,
                    ..Default::default()
                },
                Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            );
            let after =
                Headless::screenshot(renderer, size, scale, Color::TRANSPARENT);

            for (before, after) in
                bytes.chunks_exact(4).zip(after.chunks_exact(4))
            {
                let expected = 0.2 + before[3] as f64 / 255.0 * 0.8;
                assert!(
                    (after[3] as f64 / 255.0 - expected).abs() <= 2.0 / 255.0
                );
            }
        }
    }
}

fn check_insets_and_shape_changes(renderer: &mut (impl Renderer + Headless)) {
    let screen = Rectangle::with_size(Size::new(90.0, 80.0));
    let bounds = Rectangle {
        x: 13.25,
        y: 12.5,
        width: 56.0,
        height: 44.0,
    };

    // The same group ID/bounds are reused after reset. Changing just its shape
    // or reference inset must invalidate the mask, including cached GPU masks.
    for scale in [1.0f32, 1.25, 1.5] {
        for shape in [Shape::Continuous, Shape::Circular, Shape::Continuous] {
            let parent =
                Outline::new([13.25, 12.5, 56.0, 44.0], [12.0; 4], shape)
                    .unwrap();

            for inset in [0.0, 3.25, 14.0, 24.0] {
                renderer.reset(screen);
                let child = parent.inset(inset).unwrap();
                renderer.with_shaped_layer(bounds, parent, |renderer| {
                    renderer.fill_quad(
                        Quad {
                            bounds: screen,
                            snap: false,
                            ..Default::default()
                        },
                        Color::from_rgba(1.0, 1.0, 1.0, 0.4),
                    );
                    renderer.with_shaped_layer(
                        Rectangle::INFINITE,
                        child,
                        |renderer| {
                            renderer.fill_quad(
                                Quad {
                                    bounds: screen,
                                    snap: false,
                                    ..Default::default()
                                },
                                Color::from_rgba(1.0, 1.0, 1.0, 0.7),
                            );
                        },
                    );
                });
                let size =
                    Size::new((90.0 * scale) as u32, (80.0 * scale) as u32);
                let bytes = Headless::screenshot(
                    renderer,
                    size,
                    scale,
                    Color::TRANSPARENT,
                );
                let parent =
                    parent.transformed([0.0; 2], scale as f64).unwrap();
                let child = child.transformed([0.0; 2], scale as f64).unwrap();

                for y in 0..size.height {
                    for x in 0..size.width {
                        let point = [x as f64 + 0.5, y as f64 + 0.5];
                        let outer =
                            edge_coverage(parent.signed_distance(point));
                        let inner = edge_coverage(child.signed_distance(point))
                            .min(outer);
                        // The child is composed within the unmasked parent, then
                        // the parent contour is applied once to the composed group.
                        let expected = 0.4 * outer + 0.7 * inner * 0.6;
                        let actual = bytes
                            [((y * size.width + x) * 4 + 3) as usize]
                            as f64
                            / 255.0;
                        assert!(
                            (actual - expected).abs() <= 2.0 / 255.0,
                            "shape={shape:?} inset={inset} scale={scale} ({x},{y}): {actual} vs {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(all(feature = "image", feature = "geometry"))]
fn check_child_primitives<R>(renderer: &mut R)
where
    R: Headless
        + iced_wgpu::core::image::Renderer<
            Handle = iced_wgpu::core::image::Handle,
        > + iced_wgpu::core::text::Renderer<Font = Font>
        + iced_wgpu::graphics::geometry::Renderer,
{
    use iced_wgpu::core::{Point, Transformation, image, text};
    use iced_wgpu::graphics::geometry::{Frame, Path};
    let screen = Rectangle::with_size(Size::new(90.0, 80.0));
    let bounds = Rectangle {
        x: 13.25,
        y: 12.5,
        width: 56.0,
        height: 44.0,
    };
    let outline =
        Outline::new([13.25, 12.5, 56.0, 44.0], [12.0; 4], Shape::Continuous)
            .unwrap();
    let handle = image::Handle::from_rgba(1, 1, vec![255, 255, 255, 255]);

    for scale in [1.0f32, 1.25, 1.5] {
        for primitive in 0..3 {
            let draw = |renderer: &mut R| match primitive {
                0 => renderer.draw_image(
                    image::Image::new(handle.clone()).snap(false),
                    screen,
                    screen,
                ),
                1 => renderer.fill_text(
                    text::Text {
                        content: "MMMMMMMMM".into(),
                        bounds: screen.size(),
                        size: Pixels(30.0),
                        line_height: text::LineHeight::Relative(1.0),
                        font: Font::default(),
                        align_x: text::Alignment::Left,
                        align_y: iced_wgpu::core::alignment::Vertical::Top,
                        shaping: text::Shaping::Basic,
                        wrapping: text::Wrapping::None,
                        ellipsize: text::Ellipsize::None,
                    },
                    Point::new(9.0, 11.0),
                    Color::WHITE,
                    screen,
                ),
                _ => {
                    let mut frame = Frame::<R>::new(renderer, screen.size());
                    frame.fill(
                        &Path::rectangle(Point::ORIGIN, screen.size()),
                        Color::from_rgba(1.0, 1.0, 1.0, 0.7),
                    );
                    renderer.draw_geometry(frame.into_geometry());
                }
            };
            let size = Size::new((90.0 * scale) as u32, (80.0 * scale) as u32);
            renderer.reset(screen);
            renderer.with_transformation(
                Transformation::translate(0.5, 0.25),
                draw,
            );
            let plain =
                Headless::screenshot(renderer, size, scale, Color::TRANSPARENT);
            renderer.reset(screen);
            renderer.with_transformation(
                Transformation::translate(0.5, 0.25),
                |renderer| {
                    renderer.with_shaped_layer(bounds, outline, draw);
                },
            );
            let clipped =
                Headless::screenshot(renderer, size, scale, Color::TRANSPARENT);
            let outline = outline
                .transformed([0.5, 0.25], 1.0)
                .unwrap()
                .transformed([0.0; 2], scale as f64)
                .unwrap();

            for y in 0..size.height {
                for x in 0..size.width {
                    let index = ((y * size.width + x) * 4 + 3) as usize;
                    let coverage = edge_coverage(
                        outline
                            .signed_distance([x as f64 + 0.5, y as f64 + 0.5]),
                    );
                    let expected = plain[index] as f64 / 255.0 * coverage;
                    let actual = clipped[index] as f64 / 255.0;
                    assert!(
                        (expected - actual).abs() <= 2.0 / 255.0,
                        "primitive={primitive} scale={scale} ({x},{y}): {actual} vs {expected}"
                    );
                }
            }
        }
    }
}

fn check_border_clips_follow_snapped_background(
    renderer: &mut (impl Renderer + Headless),
) {
    use iced_wgpu::core::{Border, Transformation};
    let screen = Rectangle::with_size(Size::new(60.0, 60.0));
    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 26.0,
        height: 26.0,
    };
    let reference =
        Outline::new([0.25, 0.75, 27.0, 28.0], [13.0; 4], Shape::Continuous)
            .unwrap();
    for shape in [Shape::Circular, Shape::Continuous] {
        for scale in [1.0f32, 1.25, 1.5] {
            for transform in [
                Transformation::IDENTITY,
                Transformation::translate(2.25, 3.5)
                    * Transformation::scale(0.9),
            ] {
                for outline in [None, Some(reference)] {
                    for (snap, inset) in
                        [(false, 0.0), (true, 0.0), (true, 1.0), (true, 4.0)]
                    {
                        let border = Border {
                            radius: 14.0.into(),
                            width: inset,
                            color: Color::TRANSPARENT,
                            shape,
                            outline,
                            ..Default::default()
                        };
                        let size = Size::new(
                            (60.0 * scale) as u32,
                            (60.0 * scale) as u32,
                        );
                        renderer.reset(screen);
                        renderer.with_transformation(transform, |renderer| {
                            renderer.fill_quad(
                                Quad {
                                    bounds,
                                    border,
                                    snap,
                                    use_contour: true,
                                    ..Default::default()
                                },
                                Color::WHITE,
                            );
                        });
                        let background = Headless::screenshot(
                            renderer,
                            size,
                            scale,
                            Color::TRANSPARENT,
                        );
                        renderer.reset(screen);
                        renderer.with_transformation(transform, |renderer| {
                            renderer.with_border_layer(
                                bounds,
                                border,
                                snap,
                                inset,
                                |renderer| {
                                    renderer.fill_quad(
                                        Quad {
                                            bounds: screen.expand(10.0),
                                            snap: false,
                                            ..Default::default()
                                        },
                                        Color::WHITE,
                                    );
                                },
                            );
                        });
                        let child = Headless::screenshot(
                            renderer,
                            size,
                            scale,
                            Color::TRANSPARENT,
                        );
                        for (i, (bg, clip)) in background
                            .chunks_exact(4)
                            .zip(child.chunks_exact(4))
                            .enumerate()
                        {
                            assert!(
                                bg[3].abs_diff(clip[3]) <= 2,
                                "scale={scale} transform={transform:?} outline={outline:?} snap={snap} inset={inset} pixel={i}: background={} child={}",
                                bg[3],
                                clip[3]
                            );
                        }
                    }
                }
            }
        }
    }
}
