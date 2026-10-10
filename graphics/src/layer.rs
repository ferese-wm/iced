//! Draw and stack layers of graphical primitives.
use crate::core::border::Outline;
use crate::core::{Border, Rectangle, Size, Transformation};

/// One explicitly shaped content group, distinct from a rectangular clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedClip {
    pub id: u64,
    pub outline: Option<Outline>,
    pub local_border: Option<BorderClip>,
    pub bounds: Rectangle,
}

/// Local contour parameters retained until the physical pixel grid is known.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderClip {
    pub radii: [f32; 4],
    pub shape: crate::core::border::Shape,
    pub inset: f32,
    pub snap: bool,
}

impl ShapedClip {
    /// Resolves reference geometry to the renderer's physical coordinate range.
    pub fn physical(
        &self,
        scale: f32,
        size: Size<u32>,
    ) -> Option<(Outline, Rectangle)> {
        let (outline, bounds) = if let Some(border) = &self.local_border {
            let mut bounds = self.bounds * scale;
            if border.snap {
                let x = (bounds.x + 0.001).round();
                let y = (bounds.y + 0.001).round();
                let right = (bounds.x + bounds.width + 0.001).round();
                let bottom = (bounds.y + bounds.height + 0.001).round();
                bounds = Rectangle {
                    x,
                    y,
                    width: right - x,
                    height: bottom - y,
                };
            }
            let outline = if let Some(reference) = self.outline {
                reference.transformed([0.0; 2], scale as f64)?
            } else {
                Outline::new(
                    [bounds.x, bounds.y, bounds.width, bounds.height]
                        .map(f64::from),
                    border.radii.map(|r| f64::from(r * scale)),
                    border.shape,
                )?
            }
            .inset(f64::from(border.inset * scale))?;
            // Quads intersect the reference contour with their draw rectangle.
            // The inner border offsets both boundaries by the same distance.
            let bounds = bounds.shrink(border.inset * scale);
            (outline, bounds)
        } else {
            (
                self.outline?.transformed([0.0; 2], scale as f64)?,
                self.bounds * scale,
            )
        };

        if !outline
            .bounds()
            .into_iter()
            .chain(outline.radii())
            .chain([outline.inset_distance()])
            .all(|value| (value as f32).is_finite())
        {
            return None;
        }

        let viewport = Rectangle {
            x: 0.0,
            y: 0.0,
            width: size.width as f32,
            height: size.height as f32,
        };
        let bounds = bounds.intersection(&viewport.expand(0.5))?;
        Some((outline, bounds))
    }
}

/// Rectangular clips stay hard; shaped contours have a physical-pixel edge ramp.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipState {
    pub shapes: Vec<ShapedClip>,
    pub hard_bounds: Rectangle,
}

impl Default for ClipState {
    fn default() -> Self {
        Self {
            shapes: Vec::new(),
            hard_bounds: Rectangle::INFINITE,
        }
    }
}

impl ClipState {
    /// Coarse physical bounds include the antialiasing fringe of shaped contours.
    pub fn physical_bounds(
        &self,
        bounds: Rectangle,
        scale: f32,
    ) -> Option<Rectangle> {
        let bounds = bounds * scale;

        if self.shapes.is_empty() {
            Some(bounds)
        } else {
            bounds.expand(0.5).intersection(&(self.hard_bounds * scale))
        }
    }
}

/// A layer of graphical primitives.
///
/// Layers normally dictate a set of primitives that are
/// rendered in a specific order.
pub trait Layer: Default {
    /// Creates a new [`Layer`] with the given bounds.
    fn with_bounds(bounds: Rectangle) -> Self;

    /// Returns the current bounds of the [`Layer`].
    fn bounds(&self) -> Rectangle;

    /// Returns the rectangular and shaped clipping state.
    fn clips(&self) -> &ClipState;

    /// Sets the clipping state for a reused layer.
    fn set_clips(&mut self, clips: ClipState);

    /// Flushes and settles any pending group of primitives in the [`Layer`].
    ///
    /// This will be called when a [`Layer`] is finished. It allows layers to efficiently
    /// record primitives together and defer grouping until the end.
    fn flush(&mut self);

    /// Resizes the [`Layer`] to the given bounds.
    fn resize(&mut self, bounds: Rectangle);

    /// Clears all the layers contents and resets its bounds.
    fn reset(&mut self);

    /// Returns the start level of the [`Layer`].
    ///
    /// A level is a "sublayer" index inside of a [`Layer`].
    ///
    /// A [`Layer`] may draw multiple primitive types in a certain order.
    /// The level represents the lowest index of the primitive types it
    /// contains.
    ///
    /// Two layers A and B can therefore be merged if they have the same bounds,
    /// and the end level of A is lower or equal than the start level of B.
    fn start(&self) -> usize;

    /// Returns the end level of the [`Layer`].
    fn end(&self) -> usize;

    /// Merges a [`Layer`] with the current one.
    fn merge(&mut self, _layer: &mut Self);
}

/// A stack of layers used for drawing.
#[derive(Debug)]
pub struct Stack<T: Layer> {
    layers: Vec<T>,
    transformations: Vec<Transformation>,
    previous: Vec<usize>,
    current: usize,
    active_count: usize,
    next_clip: u64,
}

impl<T: Layer> Stack<T> {
    /// Creates a new empty [`Stack`].
    pub fn new() -> Self {
        Self {
            layers: vec![T::default()],
            transformations: vec![Transformation::IDENTITY],
            previous: vec![],
            current: 0,
            active_count: 1,
            next_clip: 0,
        }
    }

    /// Returns a mutable reference to the current [`Layer`] of the [`Stack`], together with
    /// the current [`Transformation`].
    #[inline]
    pub fn current_mut(&mut self) -> (&mut T, Transformation) {
        let transformation = self.transformation();

        (&mut self.layers[self.current], transformation)
    }

    /// Returns the current [`Transformation`] of the [`Stack`].
    #[inline]
    pub fn transformation(&self) -> Transformation {
        self.transformations.last().copied().unwrap()
    }

    /// Pushes a new clipping region in the [`Stack`]; creating a new layer in the
    /// process.
    pub fn push_clip(&mut self, bounds: Rectangle) {
        self.flush();
        let bounds = bounds * self.transformation();
        let mut clips = self.layers[self.current].clips().clone();
        clips.hard_bounds =
            clips
                .hard_bounds
                .intersection(&bounds)
                .unwrap_or(Rectangle {
                    width: 0.0,
                    height: 0.0,
                    ..bounds
                });
        self.push_layer(bounds, clips);
    }

    /// Starts a shaped group, retaining its reference geometry through transforms.
    pub fn push_shaped_clip(&mut self, bounds: Rectangle, outline: Outline) {
        self.push_reference_clip(bounds, Some(outline));
    }

    fn push_reference_clip(
        &mut self,
        bounds: Rectangle,
        outline: Option<Outline>,
    ) {
        self.flush();
        let transformation = self.transformation();
        let translation = transformation.translation();
        let bounds = bounds * transformation;
        let mut clips = self.layers[self.current].clips().clone();
        let outline = outline.and_then(|outline| {
            outline.transformed(
                [translation.x as f64, translation.y as f64],
                transformation.scale_factor() as f64,
            )
        });
        let id = self.next_clip;
        self.next_clip += 1;
        clips.shapes.push(ShapedClip {
            id,
            outline,
            local_border: None,
            bounds,
        });
        self.push_layer(bounds, clips);
    }

    /// Defers local radius normalization until physical snapping; reference outlines stay fixed.
    pub fn push_border_clip(
        &mut self,
        bounds: Rectangle,
        border: Border,
        snap: bool,
        inset: f32,
    ) {
        self.flush();
        let transformation = self.transformation();
        let bounds = bounds * transformation;
        let translation = transformation.translation();
        let outline = border.outline.and_then(|outline| {
            outline.transformed(
                [translation.x as f64, translation.y as f64],
                transformation.scale_factor() as f64,
            )
        });
        let mut clips = self.layers[self.current].clips().clone();
        let id = self.next_clip;
        self.next_clip += 1;
        let local_border = if border.outline.is_some() && outline.is_none() {
            None
        } else {
            Some(BorderClip {
                radii: <[f32; 4]>::from(border.radius)
                    .map(|r| r * transformation.scale_factor()),
                shape: border.shape,
                inset: inset * transformation.scale_factor(),
                snap,
            })
        };
        clips.shapes.push(ShapedClip {
            id,
            outline,
            local_border,
            bounds,
        });
        self.push_layer(bounds, clips);
    }

    fn push_layer(&mut self, bounds: Rectangle, clips: ClipState) {
        let bounds = bounds
            .intersection(&self.layers[self.current].bounds())
            .unwrap_or(Rectangle {
                width: 0.0,
                height: 0.0,
                ..bounds
            });
        self.previous.push(self.current);
        self.next_layer(bounds, clips);
    }

    fn next_layer(&mut self, bounds: Rectangle, clips: ClipState) {
        self.current = self.active_count;
        self.active_count += 1;

        if self.current == self.layers.len() {
            self.layers.push(T::with_bounds(bounds));
        } else {
            self.layers[self.current].resize(bounds);
        }

        self.layers[self.current].set_clips(clips);
    }

    /// Pops the current clipping region from the [`Stack`] and restores the previous one.
    ///
    /// The current layer will be recorded for drawing.
    pub fn pop_clip(&mut self) {
        self.flush();

        let parent = self.previous.pop().unwrap();
        let child = self.layers[self.current].clips();
        let clips = self.layers[parent].clips();

        if self.next_clip != 0
            || !clips.shapes.is_empty()
            || child.shapes != clips.shapes
        {
            // Content after a masked group must follow it in painter order.
            let bounds = self.layers[parent].bounds();
            let clips = clips.clone();
            self.next_layer(bounds, clips);
        } else {
            self.current = parent;
        }
    }

    /// Pushes a new [`Transformation`] in the [`Stack`].
    ///
    /// Future drawing operations will be affected by this new [`Transformation`] until
    /// it is popped using [`pop_transformation`].
    ///
    /// [`pop_transformation`]: Self::pop_transformation
    pub fn push_transformation(&mut self, transformation: Transformation) {
        self.transformations
            .push(self.transformation() * transformation);
    }

    /// Pops the current [`Transformation`] in the [`Stack`].
    pub fn pop_transformation(&mut self) {
        let _ = self.transformations.pop();
    }

    /// Returns an iterator over immutable references to the layers in the [`Stack`].
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.layers[..self.active_count].iter()
    }

    /// Returns the slice of layers in the [`Stack`].
    pub fn as_slice(&self) -> &[T] {
        &self.layers[..self.active_count]
    }

    /// Flushes and settles any primitives in the [`Stack`].
    pub fn flush(&mut self) {
        self.layers[self.current].flush();
    }

    /// Performs layer merging wherever possible.
    ///
    /// Flushes and settles any primitives in the [`Stack`].
    pub fn merge(&mut self) {
        self.flush();

        // These are the layers left to process
        let mut left = self.active_count;

        // There must be at least 2 or more layers to merge
        while left > 1 {
            // We set our target as the topmost layer left to process
            let mut current = left - 1;
            let mut target = &self.layers[current];
            let mut target_start = target.start();
            let mut target_index = current;

            // We scan downwards for a contiguous block of mergeable layer candidates
            while current > 0 {
                let candidate = &self.layers[current - 1];
                let start = candidate.start();
                let end = candidate.end();

                // We skip empty layers
                if end == 0 {
                    current -= 1;
                    continue;
                }

                // Candidate can be merged if primitive sublayers do not overlap with
                // previous targets and the clipping bounds match
                if end > target_start
                    || candidate.bounds() != target.bounds()
                    || candidate.clips() != target.clips()
                {
                    break;
                }

                // Candidate is not empty and can be merged into
                target = candidate;
                target_start = start;
                target_index = current;
                current -= 1;
            }

            // We merge all the layers scanned into the target
            //
            // Since we use `target_index` instead of `current`, we
            // deliberately avoid merging into empty layers.
            //
            // If no candidates were mergeable, this is a no-op.
            let (head, tail) = self.layers.split_at_mut(target_index + 1);
            let layer = &mut head[target_index];

            for middle in &mut tail[0..left - target_index - 1] {
                layer.merge(middle);
            }

            // Empty layers found after the target can be skipped
            left = current;
        }
    }

    /// Clears the layers of the [`Stack`], allowing reuse.
    ///
    /// It resizes the base layer bounds to the `new_bounds`.
    ///
    /// This will normally keep layer allocations for future drawing operations.
    pub fn reset(&mut self, new_bounds: Rectangle) {
        for layer in self.layers[..self.active_count].iter_mut() {
            layer.reset();
        }

        self.layers[0].resize(new_bounds);
        self.layers[0].set_clips(ClipState {
            hard_bounds: new_bounds,
            ..ClipState::default()
        });
        self.next_clip = 0;
        self.current = 0;
        self.active_count = 1;
        self.previous.clear();
    }
}

impl<T: Layer> Default for Stack<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::shape::Shape;

    #[derive(Default)]
    struct Recorded {
        bounds: Rectangle,
        clips: ClipState,
        marks: Vec<u8>,
        pending: Vec<u8>,
    }

    impl Layer for Recorded {
        fn with_bounds(bounds: Rectangle) -> Self {
            Self {
                bounds,
                ..Default::default()
            }
        }
        fn bounds(&self) -> Rectangle {
            self.bounds
        }
        fn clips(&self) -> &ClipState {
            &self.clips
        }
        fn set_clips(&mut self, clips: ClipState) {
            self.clips = clips;
        }
        fn flush(&mut self) {
            self.marks.append(&mut self.pending);
        }
        fn resize(&mut self, bounds: Rectangle) {
            self.bounds = bounds;
        }
        fn reset(&mut self) {
            *self = Self::default();
        }
        fn start(&self) -> usize {
            1
        }
        fn end(&self) -> usize {
            if self.marks.is_empty() { 0 } else { 1 }
        }
        fn merge(&mut self, layer: &mut Self) {
            self.marks.append(&mut layer.marks);
        }
    }

    fn outline() -> Outline {
        Outline::new([10.25, 9.5, 30.0, 25.0], [8.0; 4], Shape::Continuous)
            .unwrap()
    }

    #[test]
    fn rectangular_clips_flush_pending_content_after_a_shaped_group() {
        let bounds = Rectangle::with_size(Size::new(100.0, 90.0));
        let mut stack = Stack::<Recorded>::new();
        stack.reset(bounds);
        stack.current_mut().0.pending.push(0);
        stack.push_shaped_clip(bounds, outline());
        stack.current_mut().0.pending.push(1);
        stack.pop_clip();
        // Text and meshes are deferred until the layer is flushed. A rectangular
        // scroll or overlay clip must preserve them before leaving this layer.
        stack.current_mut().0.pending.push(2);
        stack.push_clip(bounds);
        stack.current_mut().0.pending.push(3);
        stack.push_clip(bounds);
        stack.current_mut().0.pending.push(4);
        stack.pop_clip();
        stack.current_mut().0.pending.push(5);
        stack.pop_clip();
        stack.current_mut().0.pending.push(6);
        stack.merge();
        assert_eq!(
            stack
                .iter()
                .flat_map(|layer| layer.marks.iter().copied())
                .collect::<Vec<_>>(),
            (0..7).collect::<Vec<_>>()
        );
        assert!(stack.iter().all(|layer| layer.pending.is_empty()));
    }

    #[test]
    fn shaped_groups_keep_order_and_sibling_identity_through_merge() {
        let bounds = Rectangle::with_size(Size::new(100.0, 90.0));
        let mut stack = Stack::<Recorded>::new();
        stack.reset(bounds);
        stack.current_mut().0.marks.push(0);
        stack.push_clip(bounds);
        stack.current_mut().0.marks.push(1);
        stack.push_shaped_clip(bounds, outline());
        stack.current_mut().0.marks.push(2);
        stack.pop_clip();
        stack.current_mut().0.marks.push(3);
        stack.push_shaped_clip(bounds, outline());
        stack.current_mut().0.marks.push(4);
        stack.pop_clip();
        stack.current_mut().0.marks.push(5);
        stack.pop_clip();
        stack.current_mut().0.marks.push(6);
        stack.merge();
        assert_eq!(
            stack
                .iter()
                .flat_map(|layer| layer.marks.iter().copied())
                .collect::<Vec<_>>(),
            (0..7).collect::<Vec<_>>()
        );
        let ids = stack
            .iter()
            .filter(|layer| !layer.marks.is_empty())
            .filter_map(|layer| layer.clips.shapes.last().map(|shape| shape.id))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![0, 1]);

        stack.reset(bounds);
        assert!(stack.iter().all(
            |layer| layer.clips.shapes.is_empty() && layer.marks.is_empty()
        ));
        stack.push_clip(bounds);
        stack.pop_clip();
        assert_eq!(
            stack.current, 0,
            "ordinary layers retain their previous behavior after reset"
        );
    }

    #[test]
    fn transforms_apply_once_and_shaped_fringe_respects_hard_clip() {
        let screen = Rectangle::with_size(Size::new(100.0, 90.0));
        let mut stack = Stack::<Recorded>::new();
        stack.reset(screen);
        stack.push_transformation(
            Transformation::translate(3.0, 2.0) * Transformation::scale(1.5),
        );
        let bounds = Rectangle {
            x: 10.25,
            y: 9.5,
            width: 30.0,
            height: 25.0,
        };
        stack.push_shaped_clip(bounds, outline());
        let layer = stack.current_mut().0;
        assert_eq!(
            layer.clips.shapes[0].outline,
            outline().transformed([3.0, 2.0], 1.5)
        );
        assert_eq!(
            layer.clips.physical_bounds(layer.bounds, 1.25),
            Some((layer.bounds * 1.25).expand(0.5))
        );
        stack.push_clip(bounds);
        let layer = stack.current_mut().0;
        assert_eq!(
            layer.clips.physical_bounds(layer.bounds, 1.25),
            Some(layer.bounds * 1.25)
        );
        stack.pop_clip();
        stack.pop_clip();
        stack.pop_transformation();
    }
}
