//! Visualization geometry version 1, independent of chart formats.
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Maximum coordinate on either normalized axis, inclusive.
pub const COORDINATE_MAX: u32 = 1_000_000;

/// A point on the normalized coordinate grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(rename = "VizPoint"))]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

/// An invalid geometry value; no partial geometry is produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    CoordinateOutOfRange,
    InvalidPointCount,
    InvalidRectangle,
    RepeatedClosingPoint,
}

/// An open primitive identifier and its validated normalized points.
///
/// Unknown primitive identifiers are preserved for forward-compatible fallback.
/// Consumers must check [`Geometry::is_supported`] before drawing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(rename = "VizGeometry"))]
pub struct Geometry {
    primitive: String,
    points: Vec<Point>,
}

impl Geometry {
    /// Validates known primitives and coordinate bounds, preserving unknown keys.
    pub fn new(primitive: impl Into<String>, points: Vec<Point>) -> Result<Self, GeometryError> {
        let primitive = primitive.into();
        if points
            .iter()
            .any(|p| p.x > COORDINATE_MAX || p.y > COORDINATE_MAX)
        {
            return Err(GeometryError::CoordinateOutOfRange);
        }
        match primitive.as_str() {
            "point" if points.len() != 1 => return Err(GeometryError::InvalidPointCount),
            "rect" if points.len() != 2 => return Err(GeometryError::InvalidPointCount),
            "rect" if points[0].x >= points[1].x || points[0].y >= points[1].y => {
                return Err(GeometryError::InvalidRectangle);
            }
            "polyline" if points.len() < 2 => return Err(GeometryError::InvalidPointCount),
            "polygon" if points.len() < 3 => return Err(GeometryError::InvalidPointCount),
            "polygon" if points.first() == points.last() => {
                return Err(GeometryError::RepeatedClosingPoint);
            }
            _ => {}
        }
        Ok(Self { primitive, points })
    }

    /// Returns the primitive key exactly as supplied.
    pub fn primitive(&self) -> &str {
        &self.primitive
    }

    /// Returns the validated points in their original order.
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// Whether this contract version defines the primitive's geometry.
    pub fn is_supported(&self) -> bool {
        matches!(
            self.primitive.as_str(),
            "rect" | "point" | "polyline" | "polygon"
        )
    }
}

impl<'de> Deserialize<'de> for Geometry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            primitive: String,
            points: Vec<Point>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.primitive, wire.points)
            .map_err(|error| D::Error::custom(format!("invalid geometry: {error:?}")))
    }
}
