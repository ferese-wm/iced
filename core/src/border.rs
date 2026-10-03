//! Draw lines around containers.
pub use ferese_shape::{Outline, Shape};

use crate::{Color, Pixels, Rectangle};

/// A border.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Border {
    /// The color of the border.
    pub color: Color,

    /// The width of the border.
    pub width: f32,

    /// The [`Radius`] of the border.
    pub radius: Radius,

    /// The corner profile, independent of its radii.
    pub shape: Shape,

    /// A reference outline for explicitly concentric surfaces.
    /// Coordinates are in the same space as the renderer's quad bounds.
    pub outline: Option<Outline>,
}

/// Creates a new [`Border`] with the given [`Radius`].
///
/// ```
/// # use iced_core::border::{self, Border};
/// #
/// assert_eq!(border::rounded(10), Border::default().rounded(10));
/// ```
pub fn rounded(radius: impl Into<Radius>) -> Border {
    Border::default().rounded(radius)
}

/// Creates a new [`Border`] with the given [`Color`].
///
/// ```
/// # use iced_core::border::{self, Border};
/// # use iced_core::Color;
/// #
/// assert_eq!(border::color(Color::BLACK), Border::default().color(Color::BLACK));
/// ```
pub fn color(color: impl Into<Color>) -> Border {
    Border::default().color(color)
}

/// Creates a new [`Border`] with the given `width`.
///
/// ```
/// # use iced_core::border::{self, Border};
/// # use iced_core::Color;
/// #
/// assert_eq!(border::width(10), Border::default().width(10));
/// ```
pub fn width(width: impl Into<Pixels>) -> Border {
    Border::default().width(width)
}

impl Border {
    /// Selects a profile and clears any reference outline.
    pub fn shape(self, shape: Shape) -> Self {
        Self {
            shape,
            outline: None,
            ..self
        }
    }

    /// Uses the original contour and accumulated inset of this outline.
    pub fn outline(self, outline: Outline) -> Self {
        Self {
            shape: outline.shape(),
            outline: Some(outline),
            ..self
        }
    }

    /// Resolves a reference outline or constructs an independent contour.
    pub fn outline_for(self, bounds: Rectangle) -> Option<Outline> {
        self.outline.or_else(|| {
            Outline::new(
                [bounds.x, bounds.y, bounds.width, bounds.height]
                    .map(f64::from),
                <[f32; 4]>::from(self.radius).map(f64::from),
                self.shape,
            )
        })
    }

    /// Sets the [`Color`] of the [`Border`].
    pub fn color(self, color: impl Into<Color>) -> Self {
        Self {
            color: color.into(),
            ..self
        }
    }

    /// Sets the [`Radius`] of the [`Border`].
    pub fn rounded(self, radius: impl Into<Radius>) -> Self {
        Self {
            radius: radius.into(),
            ..self
        }
    }

    /// Sets the width of the [`Border`].
    pub fn width(self, width: impl Into<Pixels>) -> Self {
        Self {
            width: width.into().0,
            ..self
        }
    }
}

/// The border radii for the corners of a graphics primitive in the order:
/// top-left, top-right, bottom-right, bottom-left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Radius {
    /// Top left radius
    pub top_left: f32,
    /// Top right radius
    pub top_right: f32,
    /// Bottom right radius
    pub bottom_right: f32,
    /// Bottom left radius
    pub bottom_left: f32,
}

/// Creates a new [`Radius`] with the same value for each corner.
pub fn radius(value: impl Into<Pixels>) -> Radius {
    Radius::new(value)
}

/// Creates a new [`Radius`] with the given top left value.
pub fn top_left(value: impl Into<Pixels>) -> Radius {
    Radius::default().top_left(value)
}

/// Creates a new [`Radius`] with the given top right value.
pub fn top_right(value: impl Into<Pixels>) -> Radius {
    Radius::default().top_right(value)
}

/// Creates a new [`Radius`] with the given bottom right value.
pub fn bottom_right(value: impl Into<Pixels>) -> Radius {
    Radius::default().bottom_right(value)
}

/// Creates a new [`Radius`] with the given bottom left value.
pub fn bottom_left(value: impl Into<Pixels>) -> Radius {
    Radius::default().bottom_left(value)
}

/// Creates a new [`Radius`] with the given value as top left and top right.
pub fn top(value: impl Into<Pixels>) -> Radius {
    Radius::default().top(value)
}

/// Creates a new [`Radius`] with the given value as bottom left and bottom right.
pub fn bottom(value: impl Into<Pixels>) -> Radius {
    Radius::default().bottom(value)
}

/// Creates a new [`Radius`] with the given value as top left and bottom left.
pub fn left(value: impl Into<Pixels>) -> Radius {
    Radius::default().left(value)
}

/// Creates a new [`Radius`] with the given value as top right and bottom right.
pub fn right(value: impl Into<Pixels>) -> Radius {
    Radius::default().right(value)
}

impl Radius {
    /// Creates a new [`Radius`] with the same value for each corner.
    pub fn new(value: impl Into<Pixels>) -> Self {
        let value = value.into().0;

        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }

    /// Sets the top left value of the [`Radius`].
    pub fn top_left(self, value: impl Into<Pixels>) -> Self {
        Self {
            top_left: value.into().0,
            ..self
        }
    }

    /// Sets the top right value of the [`Radius`].
    pub fn top_right(self, value: impl Into<Pixels>) -> Self {
        Self {
            top_right: value.into().0,
            ..self
        }
    }

    /// Sets the bottom right value of the [`Radius`].
    pub fn bottom_right(self, value: impl Into<Pixels>) -> Self {
        Self {
            bottom_right: value.into().0,
            ..self
        }
    }

    /// Sets the bottom left value of the [`Radius`].
    pub fn bottom_left(self, value: impl Into<Pixels>) -> Self {
        Self {
            bottom_left: value.into().0,
            ..self
        }
    }

    /// Sets the top left and top right values of the [`Radius`].
    pub fn top(self, value: impl Into<Pixels>) -> Self {
        let value = value.into().0;

        Self {
            top_left: value,
            top_right: value,
            ..self
        }
    }

    /// Sets the bottom left and bottom right values of the [`Radius`].
    pub fn bottom(self, value: impl Into<Pixels>) -> Self {
        let value = value.into().0;

        Self {
            bottom_left: value,
            bottom_right: value,
            ..self
        }
    }

    /// Sets the top left and bottom left values of the [`Radius`].
    pub fn left(self, value: impl Into<Pixels>) -> Self {
        let value = value.into().0;

        Self {
            top_left: value,
            bottom_left: value,
            ..self
        }
    }

    /// Sets the top right and bottom right values of the [`Radius`].
    pub fn right(self, value: impl Into<Pixels>) -> Self {
        let value = value.into().0;

        Self {
            top_right: value,
            bottom_right: value,
            ..self
        }
    }
}

impl From<f32> for Radius {
    fn from(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }
}

impl From<u8> for Radius {
    fn from(w: u8) -> Self {
        Self::from(f32::from(w))
    }
}

impl From<u32> for Radius {
    fn from(w: u32) -> Self {
        Self::from(w as f32)
    }
}

impl From<i32> for Radius {
    fn from(w: i32) -> Self {
        Self::from(w as f32)
    }
}

impl From<Radius> for [f32; 4] {
    fn from(radi: Radius) -> Self {
        [
            radi.top_left,
            radi.top_right,
            radi.bottom_right,
            radi.bottom_left,
        ]
    }
}

impl std::ops::Mul<f32> for Radius {
    type Output = Self;

    fn mul(self, scale: f32) -> Self::Output {
        Self {
            top_left: self.top_left * scale,
            top_right: self.top_right * scale,
            bottom_right: self.bottom_right * scale,
            bottom_left: self.bottom_left * scale,
        }
    }
}

impl From<[f32; 4]> for Radius {
    /// [
    ///     radi.top_left,
    ///     radi.top_right,
    ///     radi.bottom_right,
    ///     radi.bottom_left,
    /// ]
    fn from(value: [f32; 4]) -> Self {
        Self {
            top_left: value[0],
            top_right: value[1],
            bottom_right: value[2],
            bottom_left: value[3],
        }
    }
}

impl From<[u8; 4]> for Radius {
    /// [
    ///     radi.top_left,
    ///     radi.top_right,
    ///     radi.bottom_right,
    ///     radi.bottom_left,
    /// ]
    fn from(value: [u8; 4]) -> Self {
        Self {
            top_left: f32::from(value[0]),
            top_right: f32::from(value[1]),
            bottom_right: f32::from(value[2]),
            bottom_left: f32::from(value[3]),
        }
    }
}

impl From<[u16; 4]> for Radius {
    /// [
    ///     radi.top_left,
    ///     radi.top_right,
    ///     radi.bottom_right,
    ///     radi.bottom_left,
    /// ]
    fn from(value: [u16; 4]) -> Self {
        Self {
            top_left: f32::from(value[0]),
            top_right: f32::from(value[1]),
            bottom_right: f32::from(value[2]),
            bottom_left: f32::from(value[3]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_keeps_circular_geometry() {
        let border = Border::default().rounded(12);
        assert_eq!(border.shape, Shape::Circular);
        assert!(border.outline.is_none());
        let bounds = Rectangle {
            x: 2.5,
            y: 4.0,
            width: 80.0,
            height: 40.0,
        };
        let outline = border.outline_for(bounds).unwrap();
        assert_eq!(outline.bounds(), [2.5, 4.0, 80.0, 40.0]);
        assert_eq!(outline.radii(), [12.0; 4]);
    }

    #[test]
    fn concentric_child_retains_the_reference_contour() {
        let parent = Border::default().rounded(12).shape(Shape::Continuous);
        let bounds = Rectangle {
            x: 2.5,
            y: 4.0,
            width: 80.0,
            height: 40.0,
        };
        let outline = parent.outline_for(bounds).unwrap();
        let child = Border::default().outline(outline.inset(4.0).unwrap());
        let child_bounds = Rectangle {
            x: 6.5,
            y: 8.0,
            width: 72.0,
            height: 32.0,
        };
        let inner = child.outline_for(child_bounds).unwrap();
        assert_eq!(inner.bounds(), outline.bounds());
        assert_eq!(inner.radii(), outline.radii());
        assert_eq!(inner.inset_distance(), 4.0);
        assert_eq!(child.shape, Shape::Continuous);
    }

    #[test]
    fn changing_profile_explicitly_discards_a_reference_inset() {
        let outline =
            Outline::new([0.0, 0.0, 80.0, 40.0], [12.0; 4], Shape::Continuous)
                .unwrap();
        let border = Border::default()
            .outline(outline.inset(4.0).unwrap())
            .shape(Shape::Circular);
        assert_eq!(border.shape, Shape::Circular);
        assert!(border.outline.is_none());
    }
}
