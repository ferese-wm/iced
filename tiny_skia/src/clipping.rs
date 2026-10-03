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
}

#[derive(Debug, Default)]
pub(crate) struct Pipeline {
    masks: VecDeque<Arc<Mask>>,
    bytes: usize,
    pool: Vec<tiny_skia::Pixmap>,
}

impl Pipeline {
    pub(crate) fn begin(
        &mut self,
        clips: &[ShapedClip],
        size: Size<u32>,
        scale: f32,
    ) -> Option<Group> {
        let mask = self.mask(clips, size, scale);
        let mut pixmap = match self.pool.pop() {
            Some(pixmap)
                if pixmap.width() == size.width
                    && pixmap.height() == size.height =>
            {
                pixmap
            }
            _ => tiny_skia::Pixmap::new(size.width, size.height)?,
        };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        Some(Group { pixmap, mask })
    }

    pub(crate) fn finish(
        &mut self,
        groups: &mut Vec<Group>,
        pixels: &mut tiny_skia::PixmapMut<'_>,
    ) {
        let mut group = groups.pop().expect("balanced shaped groups");
        let parent = groups.last().map(|group| &group.mask);

        for (index, pixel) in
            group.pixmap.data_mut().chunks_exact_mut(4).enumerate()
        {
            let coverage = group.mask.coverage[index] as f32;
            let parent =
                parent.map_or(255.0, |mask| mask.coverage[index] as f32);
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
                0,
                0,
                group.pixmap.as_ref(),
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
        } else {
            pixels.draw_pixmap(
                0,
                0,
                group.pixmap.as_ref(),
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
        }

        self.pool.push(group.pixmap);
    }

    fn mask(
        &mut self,
        clips: &[ShapedClip],
        size: Size<u32>,
        scale: f32,
    ) -> Arc<Mask> {
        if let Some(index) = self.masks.iter().position(|mask| {
            mask.key.clips == clips
                && mask.key.size == size
                && mask.key.scale == scale
        }) {
            let mask = self.masks.remove(index).expect("cached group mask");
            self.masks.push_front(mask.clone());
            return mask;
        }

        let key = Key {
            clips: clips.to_vec(),
            size,
            scale,
        };
        let mut coverage = vec![0; size.width as usize * size.height as usize];
        let mut bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: size.width as f32,
            height: size.height as f32,
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
                coverage[y as usize * size.width as usize + x as usize] =
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_reuse_geometry_and_invalidate_for_shape_inset_and_scale() {
        let mut pipeline = Pipeline::default();
        let size = Size::new(40, 30);
        let outline =
            Outline::new([3.25, 2.5, 28.0, 23.0], [8.0; 4], Shape::Continuous)
                .unwrap();
        let clip = ShapedClip {
            id: 0,
            outline: Some(outline),
            bounds: Rectangle::INFINITE,
        };
        let first = pipeline.mask(std::slice::from_ref(&clip), size, 1.0);
        let second = pipeline.mask(std::slice::from_ref(&clip), size, 1.0);
        assert!(Arc::ptr_eq(&first, &second));
        let mut changed = clip.clone();
        changed.outline = Some(outline.inset(3.0).unwrap());
        assert!(!Arc::ptr_eq(&first, &pipeline.mask(&[changed], size, 1.0)));
        assert!(!Arc::ptr_eq(
            &first,
            &pipeline.mask(std::slice::from_ref(&clip), size, 1.25)
        ));
        changed = clip.clone();
        changed.outline =
            Outline::new(outline.bounds(), outline.radii(), Shape::Circular);
        assert!(!Arc::ptr_eq(&first, &pipeline.mask(&[changed], size, 1.0)));
        let invalid = ShapedClip {
            outline: None,
            ..clip
        };
        assert!(
            pipeline
                .mask(&[invalid], size, 1.0)
                .coverage
                .iter()
                .all(|value| *value == 0)
        );
    }
}
