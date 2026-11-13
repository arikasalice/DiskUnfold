use du_core_contracts::visualization::geometry::{COORDINATE_MAX, Geometry, GeometryError, Point};

fn p(x: u32, y: u32) -> Point {
    Point { x, y }
}

#[test]
fn valid_primitives_and_boundaries_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    for (key, points) in [
        ("point", vec![p(0, COORDINATE_MAX)]),
        ("rect", vec![p(0, 0), p(COORDINATE_MAX, COORDINATE_MAX)]),
        ("polyline", vec![p(0, 0), p(1, 1)]),
        ("polygon", vec![p(0, 0), p(1, 0), p(0, 1)]),
    ] {
        let geometry = Geometry::new(key, points.clone()).map_err(|e| format!("{e:?}"))?;
        assert!(geometry.is_supported());
        assert_eq!(geometry.points(), points);
        let encoded = serde_json::to_string(&geometry)?;
        assert_eq!(serde_json::from_str::<Geometry>(&encoded)?, geometry);
    }
    Ok(())
}

#[test]
fn rejects_invalid_known_shapes_and_coordinates_on_decode() {
    for (key, points, error) in [
        ("point", vec![], GeometryError::InvalidPointCount),
        (
            "point",
            vec![p(0, 0), p(1, 1)],
            GeometryError::InvalidPointCount,
        ),
        ("rect", vec![p(0, 0)], GeometryError::InvalidPointCount),
        (
            "rect",
            vec![p(1, 0), p(1, 1)],
            GeometryError::InvalidRectangle,
        ),
        (
            "rect",
            vec![p(0, 2), p(1, 1)],
            GeometryError::InvalidRectangle,
        ),
        ("polyline", vec![p(0, 0)], GeometryError::InvalidPointCount),
        (
            "polygon",
            vec![p(0, 0), p(1, 1)],
            GeometryError::InvalidPointCount,
        ),
        (
            "polygon",
            vec![p(0, 0), p(1, 0), p(0, 0)],
            GeometryError::RepeatedClosingPoint,
        ),
        (
            "point",
            vec![p(COORDINATE_MAX + 1, 0)],
            GeometryError::CoordinateOutOfRange,
        ),
    ] {
        assert_eq!(Geometry::new(key, points).err(), Some(error));
    }
    assert!(
        serde_json::from_str::<Geometry>(
            r#"{"primitive":"rect","points":[{"x":4,"y":0},{"x":3,"y":2}]}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<Geometry>(r#"{"primitive":"point","points":[{"x":1000001,"y":0}]}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<Geometry>(r#"{"primitive":"point","points":[{"x":-1,"y":0}]}"#)
            .is_err()
    );
}

#[test]
fn unknown_primitive_is_preserved_for_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let geometry: Geometry =
        serde_json::from_str(r#"{"primitive":"future.ribbon","points":[{"x":0,"y":1000000}]}"#)?;
    assert!(!geometry.is_supported());
    assert_eq!(geometry.primitive(), "future.ribbon");
    assert_eq!(geometry.points(), &[p(0, COORDINATE_MAX)]);
    assert_eq!(
        serde_json::from_str::<Geometry>(&serde_json::to_string(&geometry)?)?,
        geometry
    );
    assert!(
        serde_json::from_str::<Geometry>(
            r#"{"primitive":"future.ribbon","points":[{"x":0,"y":1000001}]}"#
        )
        .is_err()
    );
    Ok(())
}
