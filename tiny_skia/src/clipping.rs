use crate::core::{
    Rectangle, Size,
    shape::{Outline, Shape, edge_coverage},
};
use crate::graphics::layer::ShapedClip;
use std::collections::VecDeque;
use std::sync::Arc;

const CACHE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq)]
struct Key {
    clips: Vec<ShapedClip>,
    size: Size<u32>,
    region: Rectangle<u32>,
    scale: f32,
}

#[derive(Debug)]
struct Mask {
    key: Key,
    coverage: Vec<u8>,
}

pub(crate) struct Group {
    pub pixmap: tiny_skia::Pixmap,
    mask: Arc<Mask>,
    pub clip_mask: tiny_skia::Mask,
    pub origin: crate::core::Vector,
}

#[derive(Debug, Default)]
pub(crate) struct Pipeline {
    masks: VecDeque<Arc<Mask>>,
    bytes: usize,
    pool: Vec<(tiny_skia::Pixmap, tiny_skia::Mask)>,
    pool_bytes: usize,
}

impl Pipeline {
    pub(crate) fn begin(
        &mut self,
        clips: &[ShapedClip],
        size: Size<u32>,
        scale: f32,
        damage: Rectangle,
    ) -> Option<Group> {
        let region = region(clips, size, scale, damage).unwrap_or(Rectangle {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        });
        let mask = self.mask(clips, size, scale, region);
        let cached = self.pool.iter().position(|(pixmap, _)| {
            pixmap.width() == region.width && pixmap.height() == region.height
        });
        let (mut pixmap, clip_mask) = if let Some(index) = cached {
            let entry = self.pool.swap_remove(index);
            self.pool_bytes -= entry.0.data().len() + entry.1.data().len();
            entry
        } else {
            (
                tiny_skia::Pixmap::new(region.width, region.height)?,
                tiny_skia::Mask::new(region.width, region.height)?,
            )
        };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        Some(Group {
            pixmap,
            mask,
            clip_mask,
            origin: crate::core::Vector::new(region.x as f32, region.y as f32),
        })
    }

    pub(crate) fn finish(
        &mut self,
        groups: &mut Vec<Group>,
        pixels: &mut tiny_skia::PixmapMut<'_>,
    ) {
        let mut group = groups.pop().expect("balanced shaped groups");
        let parent = groups.last().map(|group| &group.mask);
        let region = group.mask.key.region;
        for (index, pixel) in
            group.pixmap.data_mut().chunks_exact_mut(4).enumerate()
        {
            let coverage = group.mask.coverage[index] as f32;
            let parent = parent.map_or(255.0, |mask| {
                let x = region.x + index as u32 % region.width;
                let y = region.y + index as u32 / region.width;
                let bounds = mask.key.region;
                if x < bounds.x
                    || y < bounds.y
                    || x >= bounds.x + bounds.width
                    || y >= bounds.y + bounds.height
                {
                    0.0
                } else {
                    mask.coverage[((y - bounds.y) * bounds.width + x - bounds.x)
                        as usize] as f32
                }
            });
            let relative = if parent > 0.0 {
                (coverage / parent).min(1.0)
            } else {
                0.0
            };
            for channel in pixel {
                *channel = (*channel as f32 * relative).round() as u8;
            }
        }
        let paint = tiny_skia::PixmapPaint::default();
        if let Some(parent) = groups.last_mut() {
            parent.pixmap.draw_pixmap(
                region.x as i32 - parent.mask.key.region.x as i32,
                region.y as i32 - parent.mask.key.region.y as i32,
                group.pixmap.as_ref(),
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
        } else {
            pixels.draw_pixmap(
                region.x as i32,
                region.y as i32,
                group.pixmap.as_ref(),
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
        }
        let bytes = group.pixmap.data().len() + group.clip_mask.data().len();
        if bytes <= CACHE_BYTES {
            while self.pool_bytes + bytes > CACHE_BYTES || self.pool.len() >= 32
            {
                let old = self.pool.remove(0);
                self.pool_bytes -= old.0.data().len() + old.1.data().len();
            }
            self.pool_bytes += bytes;
            self.pool.push((group.pixmap, group.clip_mask));
        }
    }

    fn mask(
        &mut self,
        clips: &[ShapedClip],
        size: Size<u32>,
        scale: f32,
        region: Rectangle<u32>,
    ) -> Arc<Mask> {
        if let Some(index) = self.masks.iter().position(|mask| {
            mask.key.clips == clips
                && mask.key.size == size
                && mask.key.scale == scale
                && mask.key.region == region
        }) {
            let mask = self.masks.remove(index).expect("cached group mask");
            self.masks.push_front(mask.clone());
            return mask;
        }

        let key = Key {
            clips: clips.to_vec(),
            size,
            region,
            scale,
        };
        let mut coverage =
            vec![0; region.width as usize * region.height as usize];
        let mut bounds = Rectangle {
            x: region.x as f32,
            y: region.y as f32,
            width: region.width as f32,
            height: region.height as f32,
        };
        let mut outlines = Vec::with_capacity(clips.len());

        for clip in clips {
            let Some((outline, clip)) = clip.physical(scale, size) else {
                return self.cache(Mask { key, coverage });
            };
            let Some(intersection) = bounds.intersection(&clip.expand(0.5))
            else {
                return self.cache(Mask { key, coverage });
            };
            bounds = intersection;
            let Some(rectangle) = Outline::new(
                [clip.x, clip.y, clip.width, clip.height].map(f64::from),
                [0.0; 4],
                Shape::Circular,
            ) else {
                return self.cache(Mask { key, coverage });
            };
            outlines.push((outline, rectangle));
        }

        for y in (bounds.y.floor() as u32)
            ..((bounds.y + bounds.height).ceil().min(size.height as f32) as u32)
        {
            for x in (bounds.x.floor() as u32)
                ..((bounds.x + bounds.width).ceil().min(size.width as f32)
                    as u32)
            {
                let point = [x as f64 + 0.5, y as f64 + 0.5];
                let distance = outlines
                    .iter()
                    .map(|(outline, rectangle)| {
                        outline
                            .signed_distance(point)
                            .max(rectangle.signed_distance(point))
                    })
                    .fold(f64::NEG_INFINITY, f64::max);
                coverage
                    [((y - region.y) * region.width + x - region.x) as usize] =
                    (edge_coverage(distance) * 255.0).round() as u8;
            }
        }

        self.cache(Mask { key, coverage })
    }

    fn cache(&mut self, mask: Mask) -> Arc<Mask> {
        let mask = Arc::new(mask);

        if mask.coverage.len() <= CACHE_BYTES {
            while self.bytes + mask.coverage.len() > CACHE_BYTES
                || self.masks.len() >= 32
            {
                let old = self.masks.pop_back().expect("clip cache budget");
                self.bytes -= old.coverage.len();
            }

            self.bytes += mask.coverage.len();
            self.masks.push_front(mask.clone());
        }

        mask
    }
}

fn region(
    clips: &[ShapedClip],
    size: Size<u32>,
    scale: f32,
    damage: Rectangle,
) -> Option<Rectangle<u32>> {
    let mut bounds = damage.intersection(&Rectangle {
        x: 0.0,
        y: 0.0,
        width: size.width as f32,
        height: size.height as f32,
    })?;
    for clip in clips {
        let (outline, clip) = clip.physical(scale, size)?;
        let [x, y, width, height] = outline.bounds().map(|v| v as f32);
        let outline_bounds = Rectangle {
            x,
            y,
            width,
            height,
        }
        .expand((-outline.inset_distance() as f32).max(0.0) + 0.5);
        bounds = bounds
            .intersection(&clip.expand(0.5))?
            .intersection(&outline_bounds)?;
    }
    let x = bounds.x.floor() as u32;
    let y = bounds.y.floor() as u32;
    Some(Rectangle {
        x,
        y,
        width: (bounds.x + bounds.width).ceil() as u32 - x,
        height: (bounds.y + bounds.height).ceil() as u32 - y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_follow_clip_and_damage_not_surface_size() {
        let mut pipeline = Pipeline::default();
        let size = Size::new(1920, 1080);
        let bounds = Rectangle {
            x: 500.0,
            y: 300.0,
            width: 100.0,
            height: 60.0,
        };
        let clip = ShapedClip {
            id: 0,
            bounds,
            outline: Outline::new(
                [500.0, 300.0, 100.0, 60.0],
                [12.0; 4],
                Shape::Continuous,
            ),
            local_border: None,
        };
        let full = pipeline
            .begin(
                std::slice::from_ref(&clip),
                size,
                1.0,
                Rectangle::with_size(Size::new(1920.0, 1080.0)),
            )
            .unwrap();
        assert_eq!((full.pixmap.width(), full.pixmap.height()), (102, 62));
        let damage = Rectangle {
            x: 520.0,
            y: 320.0,
            width: 20.0,
            height: 20.0,
        };
        let small = pipeline.begin(&[clip], size, 1.0, damage).unwrap();
        assert_eq!((small.pixmap.width(), small.pixmap.height()), (20, 20));
        assert_eq!(small.mask.coverage.len(), 400);
        for y in 0..20usize {
            for x in 0..20usize {
                assert_eq!(
                    small.mask.coverage[y * 20 + x],
                    full.mask.coverage[(y + 21) * 102 + x + 21]
                );
            }
        }
    }

    #[test]
    fn oversized_intermediates_are_not_retained() {
        let mut pipeline = Pipeline::default();
        let size = Size::new(2048, 2048);
        let mut root = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
        let bounds = Rectangle::with_size(Size::new(2048.0, 2048.0));
        let clip = ShapedClip {
            id: 0,
            bounds,
            local_border: None,
            outline: Outline::new(
                [0.0, 0.0, 2048.0, 2048.0],
                [0.0; 4],
                Shape::Circular,
            ),
        };
        let group = pipeline.begin(&[clip], size, 1.0, bounds).unwrap();
        pipeline.finish(&mut vec![group], &mut root.as_mut());
        assert_eq!(pipeline.pool_bytes, 0);
        assert!(pipeline.pool.is_empty());
    }

    #[test]
    fn masks_reuse_geometry_and_invalidate_for_shape_inset_and_scale() {
        let mut pipeline = Pipeline::default();
        let size = Size::new(40, 30);
        let region = Rectangle::with_size(size);
        let outline =
            Outline::new([3.25, 2.5, 28.0, 23.0], [8.0; 4], Shape::Continuous)
                .unwrap();
        let clip = ShapedClip {
            id: 0,
            outline: Some(outline),
            local_border: None,
            bounds: Rectangle::INFINITE,
        };
        let first =
            pipeline.mask(std::slice::from_ref(&clip), size, 1.0, region);
        let second =
            pipeline.mask(std::slice::from_ref(&clip), size, 1.0, region);
        assert!(Arc::ptr_eq(&first, &second));
        let mut changed = clip.clone();
        changed.outline = Some(outline.inset(3.0).unwrap());
        assert!(!Arc::ptr_eq(
            &first,
            &pipeline.mask(&[changed], size, 1.0, region)
        ));
        assert!(!Arc::ptr_eq(
            &first,
            &pipeline.mask(std::slice::from_ref(&clip), size, 1.25, region)
        ));
        changed = clip.clone();
        changed.outline =
            Outline::new(outline.bounds(), outline.radii(), Shape::Circular);
        assert!(!Arc::ptr_eq(
            &first,
            &pipeline.mask(&[changed], size, 1.0, region)
        ));
        let invalid = ShapedClip {
            outline: None,
            ..clip
        };
        assert!(
            pipeline
                .mask(&[invalid], size, 1.0, region)
                .coverage
                .iter()
                .all(|value| *value == 0)
        );
    }
}
