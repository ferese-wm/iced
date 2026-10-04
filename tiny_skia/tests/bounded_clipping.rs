use iced_tiny_skia::{
    Renderer,
    core::{
        Color, Font, Pixels, Rectangle, Renderer as _, Size,
        border::{Outline, Shape},
        renderer::Quad,
    },
    graphics::Viewport,
};

fn scene(renderer: &mut Renderer, screen: Rectangle, count: usize) {
    renderer.reset(screen);
    for i in 0..count {
        let bounds = Rectangle {
            x: 500.25,
            y: 100.5 + i as f32 * 100.0,
            width: 100.0,
            height: 60.0,
        };
        let outline = Outline::new(
            [bounds.x, bounds.y, bounds.width, bounds.height].map(f64::from),
            [12.0; 4],
            Shape::Continuous,
        )
        .unwrap();
        renderer.with_shaped_layer(bounds, outline, |renderer| {
            renderer.fill_quad(
                Quad {
                    bounds: screen,
                    snap: false,
                    ..Default::default()
                },
                Color::from_rgba(0.3, 0.6, 0.9, 0.7),
            );
        });
    }
}

#[test]
fn partial_damage_matches_full_render_and_preserves_undamaged_pixels() {
    for scale in [1.0, 1.25, 1.5] {
        let mut renderer = Renderer::new(Font::default(), Pixels(16.0));
        let size = Size::new(960, 540);
        let viewport = Viewport::with_physical_size(size, scale);
        let screen = Rectangle::with_size(viewport.logical_size());
        scene(&mut renderer, screen, 3);
        let mut full = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
        let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
        renderer.draw(
            &mut full.as_mut(),
            &mut mask,
            &viewport,
            &[screen],
            Color::TRANSPARENT,
        );
        // Include a corner and a sibling; damage coordinates are integer physical pixels.
        let damage = Rectangle {
            x: 500.0 * scale as f32,
            y: 98.0 * scale as f32,
            width: 30.0,
            height: 150.0,
        };
        let logical_damage = damage * (1.0 / scale as f32);
        let mut partial =
            tiny_skia::Pixmap::new(size.width, size.height).unwrap();
        partial.fill(tiny_skia::Color::from_rgba8(17, 23, 31, 255));
        renderer.draw(
            &mut partial.as_mut(),
            &mut mask,
            &viewport,
            &[logical_damage],
            Color::TRANSPARENT,
        );
        for y in 0..size.height {
            for x in 0..size.width {
                let inside = x as f32 >= damage.x
                    && (x as f32) < damage.x + damage.width
                    && y as f32 >= damage.y
                    && (y as f32) < damage.y + damage.height;
                let i = ((y * size.width + x) * 4) as usize;
                let expected = if inside {
                    &full.data()[i..i + 4]
                } else {
                    &[17, 23, 31, 255]
                };
                assert_eq!(
                    &partial.data()[i..i + 4],
                    expected,
                    "scale={scale} ({x},{y})"
                );
            }
        }
    }
}

fn assert_partial_matches_full(
    renderer: &mut Renderer,
    viewport: &Viewport,
    full: &tiny_skia::Pixmap,
    damage: Rectangle,
) {
    let size = viewport.physical_size();
    let mut partial = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
    partial.fill(tiny_skia::Color::from_rgba8(17, 23, 31, 255));
    let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
    renderer.draw(
        &mut partial.as_mut(),
        &mut mask,
        viewport,
        &[damage * (1.0 / viewport.scale_factor() as f32)],
        Color::TRANSPARENT,
    );

    for y in 0..size.height {
        for x in 0..size.width {
            let inside = x as f32 >= damage.x
                && (x as f32) < damage.x + damage.width
                && y as f32 >= damage.y
                && (y as f32) < damage.y + damage.height;
            let i = ((y * size.width + x) * 4) as usize;
            let expected = if inside {
                &full.data()[i..i + 4]
            } else {
                &[17, 23, 31, 255]
            };
            assert_eq!(
                &partial.data()[i..i + 4],
                expected,
                "scale={} damage={damage:?} ({x},{y})",
                viewport.scale_factor(),
            );
        }
    }
}

#[test]
fn nested_crops_with_different_origins_preserve_coverage_and_partial_damage() {
    use iced_tiny_skia::core::shape::edge_coverage;

    let parent_bounds = Rectangle {
        x: 27.25,
        y: 22.5,
        width: 154.0,
        height: 116.0,
    };
    let child_bounds = Rectangle {
        x: 69.5,
        y: 43.25,
        width: 91.0,
        height: 64.0,
    };

    for scale in [1.0, 1.25, 1.5] {
        for shape in [Shape::Circular, Shape::Continuous] {
            let viewport =
                Viewport::with_physical_size(Size::new(320, 240), scale);
            let screen = Rectangle::with_size(viewport.logical_size());
            let parent = Outline::new(
                [
                    parent_bounds.x,
                    parent_bounds.y,
                    parent_bounds.width,
                    parent_bounds.height,
                ]
                .map(f64::from),
                [23.0; 4],
                shape,
            )
            .unwrap();
            let child = Outline::new(
                [
                    child_bounds.x,
                    child_bounds.y,
                    child_bounds.width,
                    child_bounds.height,
                ]
                .map(f64::from),
                [18.0; 4],
                shape,
            )
            .unwrap();
            let mut renderer = Renderer::new(Font::default(), Pixels(16.0));
            renderer.reset(screen);
            renderer.with_shaped_layer(parent_bounds, parent, |renderer| {
                renderer.fill_quad(
                    Quad {
                        bounds: screen,
                        snap: false,
                        ..Default::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, 0.4),
                );
                renderer.with_shaped_layer(child_bounds, child, |renderer| {
                    renderer.fill_quad(
                        Quad {
                            bounds: screen,
                            snap: false,
                            ..Default::default()
                        },
                        Color::from_rgba(1.0, 1.0, 1.0, 0.7),
                    );
                });
            });
            let mut full = tiny_skia::Pixmap::new(320, 240).unwrap();
            let mut mask = tiny_skia::Mask::new(320, 240).unwrap();
            renderer.draw(
                &mut full.as_mut(),
                &mut mask,
                &viewport,
                &[screen],
                Color::TRANSPARENT,
            );
            let parent = parent.transformed([0.0; 2], scale).unwrap();
            let child = child.transformed([0.0; 2], scale).unwrap();

            for y in 0..240 {
                for x in 0..320 {
                    let point = [x as f64 + 0.5, y as f64 + 0.5];
                    let outer = edge_coverage(parent.signed_distance(point));
                    let inner =
                        edge_coverage(child.signed_distance(point)).min(outer);
                    let expected = (0.4 * outer + 0.7 * inner * 0.6) * 255.0;
                    let actual = full.data()[((y * 320 + x) * 4 + 3) as usize];
                    assert!(
                        (actual as f64 - expected).abs() <= 2.0,
                        "scale={scale} shape={shape:?} ({x},{y}): {actual} vs {expected}",
                    );
                }
            }

            // One region cuts the nested contours; the other begins inside the
            // parent while the child retains its distinct physical origin.
            for damage in [
                Rectangle {
                    x: 30.0,
                    y: 15.0,
                    width: 150.0,
                    height: 105.0,
                },
                Rectangle {
                    x: 90.0,
                    y: 60.0,
                    width: 120.0,
                    height: 120.0,
                },
            ] {
                assert_partial_matches_full(
                    &mut renderer,
                    &viewport,
                    &full,
                    damage,
                );
            }
        }
    }
}

#[test]
fn text_crossing_crop_edges_keeps_surface_hinting_and_partial_damage() {
    use iced_tiny_skia::core::text::Renderer as _;
    use iced_tiny_skia::core::{Point, alignment, shape::edge_coverage, text};

    let bounds = Rectangle {
        x: 41.25,
        y: 25.5,
        width: 135.0,
        height: 80.0,
    };

    for scale in [1.0, 1.25, 1.5] {
        for (shape, radius) in
            [(Shape::Circular, 0.0), (Shape::Continuous, 12.0)]
        {
            let viewport =
                Viewport::with_physical_size(Size::new(320, 240), scale);
            let screen = Rectangle::with_size(viewport.logical_size());
            let mut renderer = Renderer::new(Font::default(), Pixels(16.0));
            let draw = |renderer: &mut Renderer| {
                renderer.fill_text(
                    text::Text {
                        content: "MMMMMMMMM\nMMMMMMMMM\nMMMMMMMMM".into(),
                        bounds: screen.size(),
                        size: Pixels(28.0),
                        line_height: text::LineHeight::Relative(1.2),
                        font: Font::default(),
                        align_x: text::Alignment::Left,
                        align_y: alignment::Vertical::Top,
                        shaping: text::Shaping::Basic,
                        wrapping: text::Wrapping::None,
                        ellipsize: text::Ellipsize::None,
                    },
                    Point::new(29.125, 18.375),
                    Color::WHITE,
                    screen,
                );
            };
            let mut mask = tiny_skia::Mask::new(320, 240).unwrap();
            renderer.reset(screen);
            draw(&mut renderer);
            let mut plain = tiny_skia::Pixmap::new(320, 240).unwrap();
            renderer.draw(
                &mut plain.as_mut(),
                &mut mask,
                &viewport,
                &[screen],
                Color::TRANSPARENT,
            );
            let outline = Outline::new(
                [bounds.x, bounds.y, bounds.width, bounds.height]
                    .map(f64::from),
                [radius; 4],
                shape,
            )
            .unwrap();
            renderer.reset(screen);
            renderer.with_shaped_layer(bounds, outline, draw);
            let mut clipped = tiny_skia::Pixmap::new(320, 240).unwrap();
            renderer.draw(
                &mut clipped.as_mut(),
                &mut mask,
                &viewport,
                &[screen],
                Color::TRANSPARENT,
            );
            let physical = bounds * scale as f32;
            let top = physical.y.floor() as u32;
            let left = physical.x.floor() as u32;
            assert!(
                (left..(physical.x + physical.width).ceil() as u32)
                    .any(|x| plain.data()[((top * 320 + x) * 4 + 3) as usize]
                        != 0),
                "text must cross crop top"
            );
            assert!(
                (top..(physical.y + physical.height).ceil() as u32)
                    .any(|y| plain.data()[((y * 320 + left) * 4 + 3) as usize]
                        != 0),
                "text must cross crop left"
            );
            let outline = outline.transformed([0.0; 2], scale).unwrap();

            for y in 0..240 {
                for x in 0..320 {
                    let coverage =
                        (edge_coverage(outline.signed_distance([
                            x as f64 + 0.5,
                            y as f64 + 0.5,
                        ])) * 255.0)
                            .round();
                    let i = ((y * 320 + x) * 4) as usize;
                    for channel in 0..4 {
                        let expected = (plain.data()[i + channel] as f64
                            * coverage
                            / 255.0)
                            .round();
                        let actual = clipped.data()[i + channel];
                        assert!(
                            (actual as f64 - expected).abs() <= 1.0,
                            "scale={scale} shape={shape:?} ({x},{y}) channel={channel}: {actual} vs {expected}",
                        );
                    }
                }
            }

            for damage in [
                Rectangle {
                    x: 45.0,
                    y: 30.0,
                    width: 75.0,
                    height: 60.0,
                },
                Rectangle {
                    x: 60.0,
                    y: 45.0,
                    width: 105.0,
                    height: 75.0,
                },
            ] {
                assert_partial_matches_full(
                    &mut renderer,
                    &viewport,
                    &clipped,
                    damage,
                );
            }
        }
    }
}

#[test]
#[ignore = "release rendering benchmark; timings depend on host"]
fn profile_small_clips() {
    let mut renderer = Renderer::new(Font::default(), Pixels(16.0));
    let size = Size::new(1920, 1080);
    let viewport = Viewport::with_physical_size(size, 1.0);
    let screen = Rectangle::with_size(viewport.logical_size());
    let mut pixels = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
    let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
    for count in [1, 6] {
        scene(&mut renderer, screen, count);
        for (name, damage) in [
            ("full", screen),
            (
                "20x20",
                Rectangle {
                    x: 520.0,
                    y: 120.0,
                    width: 20.0,
                    height: 20.0,
                },
            ),
        ] {
            for _ in 0..3 {
                renderer.draw(
                    &mut pixels.as_mut(),
                    &mut mask,
                    &viewport,
                    &[damage],
                    Color::TRANSPARENT,
                );
            }
            let start = std::time::Instant::now();
            for _ in 0..20 {
                renderer.draw(
                    &mut pixels.as_mut(),
                    &mut mask,
                    &viewport,
                    &[damage],
                    Color::TRANSPARENT,
                );
            }
            eprintln!(
                "clips={count} damage={name}: {:.3} ms/frame",
                start.elapsed().as_secs_f64() * 1000.0 / 20.0
            );
        }
    }
}

#[cfg(feature = "image")]
#[test]
#[ignore = "release rendering benchmark; timings depend on host"]
fn profile_plain_image() {
    use iced_tiny_skia::core::image::{self, Renderer as _};
    let mut renderer = Renderer::new(Font::default(), Pixels(16.0));
    let viewport = Viewport::with_physical_size(Size::new(1920, 1080), 1.0);
    let screen = Rectangle::with_size(viewport.logical_size());
    let handle =
        image::Handle::from_rgba(1920, 1080, vec![200; 1920 * 1080 * 4]);
    renderer.reset(screen);
    renderer.draw_image(image::Image::new(handle), screen, screen);
    let mut pixels = tiny_skia::Pixmap::new(1920, 1080).unwrap();
    let mut mask = tiny_skia::Mask::new(1920, 1080).unwrap();
    for _ in 0..3 {
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &viewport,
            &[screen],
            Color::TRANSPARENT,
        );
    }
    let start = std::time::Instant::now();
    for _ in 0..20 {
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &viewport,
            &[screen],
            Color::TRANSPARENT,
        );
    }
    eprintln!(
        "plain image: {:.3} ms/frame",
        start.elapsed().as_secs_f64() * 1000.0 / 20.0
    );
}
