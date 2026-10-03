use crate::core::image as raster;
use crate::core::shape::{Outline, edge_coverage};
use crate::core::{Rectangle, Size};
use crate::graphics;
use std::collections::VecDeque;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::collections::hash_map;

#[derive(Debug)]
pub struct Pipeline {
    cache: RefCell<Cache>,
    masks: VecDeque<Arc<ClipMask>>,
    mask_bytes: usize,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            cache: RefCell::new(Cache::default()),
            masks: VecDeque::new(),
            mask_bytes: 0,
        }
    }

    pub fn load(
        &self,
        handle: &raster::Handle,
    ) -> Result<raster::Allocation, raster::Error> {
        let mut cache = self.cache.borrow_mut();
        let image = cache.allocate(handle)?;

        #[allow(unsafe_code)]
        Ok(unsafe {
            raster::allocate(handle, Size::new(image.width(), image.height()))
        })
    }

    pub fn dimensions(&self, handle: &raster::Handle) -> Option<Size<u32>> {
        let mut cache = self.cache.borrow_mut();
        let image = cache.allocate(handle).ok()?;

        Some(Size::new(image.width(), image.height()))
    }

    pub fn draw(
        &mut self,
        image: &raster::Image,
        bounds: Rectangle,
        outline: Outline,
        clip_bounds: Rectangle,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &tiny_skia::Mask,
        damage_bounds: Rectangle,
    ) {
        let Some(region) = clip_bounds
            .expand(1.0)
            .intersection(&damage_bounds)
            .and_then(|bounds| {
                bounds.intersection(&Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: pixels.width() as f32,
                    height: pixels.height() as f32,
                })
            })
        else {
            return;
        };
        let left = region.x.floor() as u32;
        let top = region.y.floor() as u32;
        let right =
            (region.x + region.width).ceil().min(pixels.width() as f32) as u32;
        let bottom = (region.y + region.height)
            .ceil()
            .min(pixels.height() as f32) as u32;
        let Some(mut target) =
            tiny_skia::Pixmap::new(right - left, bottom - top)
        else {
            return;
        };
        let Some(local_outline) =
            outline.transformed([-(left as f64), -(top as f64)], 1.0)
        else {
            return;
        };
        let key = ClipKey {
            outline: local_outline,
            bounds: [
                clip_bounds.x as f64 - left as f64,
                clip_bounds.y as f64 - top as f64,
                clip_bounds.width as f64,
                clip_bounds.height as f64,
            ],
            size: [right - left, bottom - top],
            image_bounds: [
                bounds.x as f64 - left as f64,
                bounds.y as f64 - top as f64,
                bounds.width as f64,
                bounds.height as f64,
            ],
            rotation: f32::from(image.rotation),
        };
        let mask = self.mask(key);
        let mut cache = self.cache.borrow_mut();
        let resample = {
            let Ok(decoded) = cache.allocate(&image.handle) else {
                return;
            };
            graphics::image::downsample_target(
                Size::new(decoded.width(), decoded.height()),
                bounds.size(),
            )
        };
        let Ok(decoded) = (match resample {
            Some(size) => cache.allocate_resampled(&image.handle, size),
            None => cache.allocate(&image.handle),
        }) else {
            return;
        };
        let center = bounds.center();
        let transform = tiny_skia::Transform::from_scale(
            bounds.width / decoded.width() as f32,
            bounds.height / decoded.height() as f32,
        )
        .post_translate(bounds.x - left as f32, bounds.y - top as f32)
        .post_rotate_at(
            -f32::from(image.rotation).to_degrees(),
            center.x - left as f32,
            center.y - top as f32,
        );
        let quality = match image.filter_method {
            raster::FilterMethod::Linear => tiny_skia::FilterQuality::Bilinear,
            raster::FilterMethod::Nearest => tiny_skia::FilterQuality::Nearest,
        };
        let shader = tiny_skia::Pattern::new(
            decoded,
            tiny_skia::SpreadMode::Pad,
            quality,
            image.opacity,
            transform,
        );
        target.fill_rect(
            tiny_skia::Rect::from_xywh(
                0.0,
                0.0,
                target.width() as f32,
                target.height() as f32,
            )
            .expect("valid raster region"),
            &tiny_skia::Paint {
                shader,
                anti_alias: false,
                ..Default::default()
            },
            tiny_skia::Transform::identity(),
            None,
        );

        for (pixel, coverage) in
            target.data_mut().chunks_exact_mut(4).zip(&mask.coverage)
        {
            for channel in pixel {
                *channel =
                    ((*channel as u32 * *coverage as u32 + 127) / 255) as u8;
            }
        }

        pixels.draw_pixmap(
            left as i32,
            top as i32,
            target.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            Some(clip_mask),
        );
    }

    fn mask(&mut self, key: ClipKey) -> Arc<ClipMask> {
        if let Some(index) = self.masks.iter().position(|mask| mask.key == key)
        {
            let mask = self.masks.remove(index).expect("cached clip mask");
            self.masks.push_front(mask.clone());
            return mask;
        }

        let bounds = Outline::new(
            key.bounds,
            [0.0; 4],
            crate::core::border::Shape::Circular,
        )
        .expect("valid image clip");
        let image = Outline::new(
            key.image_bounds,
            [0.0; 4],
            crate::core::border::Shape::Circular,
        )
        .expect("valid image bounds");
        let center = [
            key.image_bounds[0] + key.image_bounds[2] * 0.5,
            key.image_bounds[1] + key.image_bounds[3] * 0.5,
        ];
        let (sin, cos) = (key.rotation as f64).sin_cos();
        let mut coverage =
            Vec::with_capacity(key.size[0] as usize * key.size[1] as usize);

        for y in 0..key.size[1] {
            for x in 0..key.size[0] {
                let point = [x as f64 + 0.5, y as f64 + 0.5];
                let delta = [point[0] - center[0], point[1] - center[1]];
                let unrotated = [
                    delta[0] * cos - delta[1] * sin + center[0],
                    delta[0] * sin + delta[1] * cos + center[1],
                ];
                let distance = bounds
                    .signed_distance(point)
                    .max(key.outline.signed_distance(point))
                    .max(image.signed_distance(unrotated));
                coverage.push((edge_coverage(distance) * 255.0).round() as u8);
            }
        }

        let mask = Arc::new(ClipMask { key, coverage });
        const BUDGET: usize = 16 * 1024 * 1024;

        if mask.coverage.len() <= BUDGET {
            while self.mask_bytes + mask.coverage.len() > BUDGET
                || self.masks.len() >= 32
            {
                let old =
                    self.masks.pop_back().expect("clip mask cache budget");
                self.mask_bytes -= old.coverage.len();
            }

            self.mask_bytes += mask.coverage.len();
            self.masks.push_front(mask.clone());
        }

        mask
    }

    pub fn trim_cache(&mut self) {
        self.cache.borrow_mut().trim();
    }
}

#[derive(Debug, Default)]
struct Cache {
    entries: FxHashMap<raster::Id, Option<Entry>>,
    hits: FxHashSet<raster::Id>,
    resampled: FxHashMap<(raster::Id, u32, u32), Entry>,
    resampled_hits: FxHashSet<(raster::Id, u32, u32)>,
}

impl Cache {
    pub fn allocate(
        &mut self,
        handle: &raster::Handle,
    ) -> Result<tiny_skia::PixmapRef<'_>, raster::Error> {
        let id = handle.id();

        if let hash_map::Entry::Vacant(entry) = self.entries.entry(id) {
            let image = match graphics::image::load(handle) {
                Ok(image) => image,
                Err(error) => {
                    let _ = entry.insert(None);

                    return Err(error);
                }
            };

            if image.width() == 0 || image.height() == 0 {
                return Err(raster::Error::Empty);
            }

            let mut buffer =
                vec![0u32; image.width() as usize * image.height() as usize];

            for (i, pixel) in image.pixels().enumerate() {
                let [r, g, b, a] = pixel.0;

                buffer[i] = bytemuck::cast(
                    tiny_skia::ColorU8::from_rgba(b, g, r, a).premultiply(),
                );
            }

            let _ = entry.insert(Some(Entry {
                width: image.width(),
                height: image.height(),
                pixels: buffer,
            }));
        }

        let _ = self.hits.insert(id);
        let Some(ret) = self.entries.get(&id).unwrap().as_ref().map(|entry| {
            tiny_skia::PixmapRef::from_bytes(
                bytemuck::cast_slice(&entry.pixels),
                entry.width,
                entry.height,
            )
            .expect("Build pixmap from image bytes")
        }) else {
            return Err(raster::Error::Empty);
        };

        Ok(ret)
    }

    /// Like [`Self::allocate`], resampled to `target`. Call after
    /// [`Self::allocate`] has decoded the image.
    pub fn allocate_resampled(
        &mut self,
        handle: &raster::Handle,
        target: Size<u32>,
    ) -> Result<tiny_skia::PixmapRef<'_>, raster::Error> {
        let key = (handle.id(), target.width, target.height);

        if !self.resampled.contains_key(&key) {
            let native = self
                .entries
                .get(&handle.id())
                .and_then(Option::as_ref)
                .ok_or(raster::Error::Empty)?;

            // Stored pixels are already premultiplied, so resample them as is.
            let pixels = graphics::image::downsample_premultiplied(
                bytemuck::cast_slice(&native.pixels),
                Size::new(native.width, native.height),
                target,
            );

            let _ = self.resampled.insert(
                key,
                Entry {
                    width: target.width,
                    height: target.height,
                    pixels: pixels
                        .chunks_exact(4)
                        .map(|p| u32::from_ne_bytes([p[0], p[1], p[2], p[3]]))
                        .collect(),
                },
            );
        }

        let _ = self.resampled_hits.insert(key);
        let entry = &self.resampled[&key];

        Ok(tiny_skia::PixmapRef::from_bytes(
            bytemuck::cast_slice(&entry.pixels),
            entry.width,
            entry.height,
        )
        .expect("Build pixmap from image bytes"))
    }

    fn trim(&mut self) {
        self.entries.retain(|key, _| self.hits.contains(key));
        self.resampled
            .retain(|key, _| self.resampled_hits.contains(key));
        self.hits.clear();
        self.resampled_hits.clear();
    }
}

#[derive(Debug)]
struct Entry {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
struct ClipKey {
    outline: Outline,
    bounds: [f64; 4],
    size: [u32; 2],
    image_bounds: [f64; 4],
    rotation: f32,
}

#[derive(Debug)]
struct ClipMask {
    key: ClipKey,
    coverage: Vec<u8>,
}
