use du_analysis_index::visualization::squarify::{LayoutError, Rect, SkipReason, Weight, layout};

fn rectangle(left: u32, top: u32, right: u32, bottom: u32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}
fn area(rect: Rect) -> u64 {
    u64::from(rect.right - rect.left) * u64::from(rect.bottom - rect.top)
}
fn overlap(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}
fn assert_valid(parent: Rect, weights: &[Weight]) -> Result<(), LayoutError> {
    let first = layout(parent, weights)?;
    assert_eq!(first, layout(parent, weights)?);
    for (index, shape) in first.shapes.iter().enumerate() {
        assert!(shape.rect.left >= parent.left && shape.rect.top >= parent.top);
        assert!(shape.rect.right <= parent.right && shape.rect.bottom <= parent.bottom);
        assert!(area(shape.rect) > 0);
        assert_eq!(shape.weight, weights[shape.input_index]);
        assert!(
            first.shapes[index + 1..]
                .iter()
                .all(|other| !overlap(shape.rect, other.rect))
        );
    }
    assert_eq!(first.shapes.len() + first.skipped.len(), weights.len());
    Ok(())
}

#[test]
fn follows_squarified_rows_in_confirmed_order() -> Result<(), LayoutError> {
    let parent = rectangle(0, 0, 100, 100);
    let result = layout(
        parent,
        &[
            Weight::Exact(6),
            Weight::Exact(6),
            Weight::Exact(4),
            Weight::Exact(4),
        ],
    )?;
    let rectangles: Vec<_> = result
        .shapes
        .iter()
        .map(|shape| (shape.input_index, shape.rect))
        .collect();
    assert_eq!(
        rectangles,
        vec![
            (0, rectangle(0, 0, 60, 50)),
            (1, rectangle(0, 50, 60, 100)),
            (2, rectangle(60, 0, 100, 50)),
            (3, rectangle(60, 50, 100, 100))
        ]
    );
    assert!(result.skipped.is_empty());
    assert_eq!(
        result.shapes.iter().map(|s| area(s.rect)).sum::<u64>(),
        area(parent)
    );
    Ok(())
}

#[test]
fn unknown_and_zero_have_no_area_or_effect_on_positive_layout() -> Result<(), LayoutError> {
    let parent = rectangle(4, 7, 104, 107);
    let input = [
        Weight::Exact(60),
        Weight::Unknown,
        Weight::Exact(0),
        Weight::LowerBound(0),
        Weight::LowerBound(40),
    ];
    let result = layout(parent, &input)?;
    assert_eq!(result.shapes.len(), 2);
    assert_eq!(result.shapes[0].rect, rectangle(4, 7, 64, 107));
    assert_eq!(result.shapes[1].rect, rectangle(64, 7, 104, 107));
    assert_eq!(result.shapes[1].weight, Weight::LowerBound(40));
    assert_eq!(
        result
            .skipped
            .iter()
            .map(|v| (v.input_index, v.reason))
            .collect::<Vec<_>>(),
        vec![
            (1, SkipReason::Unknown),
            (2, SkipReason::ExactZero),
            (3, SkipReason::LowerBoundZero)
        ]
    );
    Ok(())
}

#[test]
fn boundary_and_precision_losses_are_explicit() -> Result<(), LayoutError> {
    let parent = rectangle(0, 0, 1, 1);
    let result = layout(
        parent,
        &[Weight::Exact(u64::MAX), Weight::Exact(1), Weight::Unknown],
    )?;
    assert_eq!(result.shapes.len(), 1);
    assert_eq!(result.shapes[0].rect, parent);
    assert_eq!(result.skipped[0].reason, SkipReason::CoordinatePrecision);
    assert_eq!(result.skipped[1].reason, SkipReason::Unknown);
    assert_eq!(
        layout(rectangle(0, 0, 0, 1), &[]),
        Err(LayoutError::InvalidBounds)
    );
    assert_eq!(
        layout(
            rectangle(0, 0, 10, 10),
            &[Weight::Exact(1), Weight::Exact(2)]
        ),
        Err(LayoutError::NotDescending)
    );
    let empty = layout(parent, &[Weight::Unknown, Weight::Exact(0)])?;
    assert!(empty.shapes.is_empty());
    assert_eq!(empty.skipped.len(), 2);
    Ok(())
}

#[test]
fn varied_parents_keep_edges_disjoint_and_deterministic() -> Result<(), LayoutError> {
    let cases = [
        rectangle(5, 10, 44, 403),
        rectangle(2, 3, 503, 58),
        rectangle(0, 0, 1000, 1000),
        rectangle(u32::MAX - 101, u32::MAX - 63, u32::MAX, u32::MAX),
    ];
    let values: Vec<_> = (1..=100).rev().map(|n| Weight::Exact(n * n)).collect();
    for rect in cases {
        assert_valid(rect, &values)?;
    }
    assert_valid(
        rectangle(0, 0, u32::MAX, u32::MAX),
        &[
            Weight::Exact(u64::MAX),
            Weight::Exact(u64::MAX),
            Weight::Exact(u64::MAX - 1),
        ],
    )?;
    Ok(())
}

#[test]
fn generated_sorted_inputs_preserve_rectangular_invariants() -> Result<(), LayoutError> {
    let mut state = 0x57b0_9c3d_u64;
    for _ in 0..256 {
        let mut numbers = Vec::new();
        for _ in 0..40 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            numbers.push((state >> 8) % 100_000 + 1);
        }
        numbers.sort_unstable_by(|a, b| b.cmp(a));
        let weights: Vec<_> = numbers.into_iter().map(Weight::Exact).collect();
        let parent = rectangle(
            9,
            17,
            10 + (state as u32 % 150),
            18 + ((state >> 32) as u32 % 150),
        );
        assert_valid(parent, &weights)?;
        let result = layout(parent, &weights)?;
        if result.skipped.is_empty() {
            assert_eq!(
                result
                    .shapes
                    .iter()
                    .map(|shape| area(shape.rect))
                    .sum::<u64>(),
                area(parent)
            );
        }
    }
    Ok(())
}

#[test]
fn cancellable_layout_preserves_results_and_never_returns_partial_at_any_checkpoint()
-> Result<(), LayoutError> {
    use du_analysis_index::visualization::squarify::layout_cancellable;
    let cases = [
        (
            rectangle(0, 0, 100, 100),
            vec![
                Weight::Exact(20),
                Weight::Exact(10),
                Weight::Exact(5),
                Weight::Exact(0),
                Weight::Unknown,
                Weight::LowerBound(0),
            ],
        ),
        (
            rectangle(0, 0, 1, 1),
            vec![
                Weight::Exact(100),
                Weight::Exact(10),
                Weight::Exact(1),
                Weight::Exact(1),
            ],
        ),
        (rectangle(0, 0, 100, 100), vec![]),
    ];
    for (bounds, weights) in cases {
        let expected = layout(bounds, &weights)?;
        let mut checks = 0;
        let actual = layout_cancellable(bounds, &weights, &mut || {
            checks += 1;
            false
        })?;
        assert_eq!(actual, expected);
        for stop in 1..=checks {
            let mut seen = 0;
            assert_eq!(
                layout_cancellable(bounds, &weights, &mut || {
                    seen += 1;
                    seen == stop
                }),
                Err(LayoutError::Cancelled),
                "stop {stop} of {checks}"
            );
        }
    }
    Ok(())
}

#[test]
fn cancellation_interrupts_large_skipped_ordering() {
    use du_analysis_index::visualization::squarify::layout_cancellable;
    let mut weights = vec![Weight::Exact(0); 8192];
    weights[0] = Weight::Exact(1);
    weights[4096] = Weight::Exact(1);
    let mut checks = 0;
    let result = layout_cancellable(rectangle(0, 0, 1, 1), &weights, &mut || {
        checks += 1;
        checks == weights.len() + 100
    });
    assert!(matches!(result, Err(LayoutError::Cancelled)));
}

#[test]
fn skipped_zero_and_precision_entries_merge_in_original_order() -> Result<(), LayoutError> {
    let input = [
        Weight::Exact(5),
        Weight::Exact(0),
        Weight::Exact(4),
        Weight::Unknown,
        Weight::Exact(3),
        Weight::LowerBound(0),
        Weight::Exact(2),
        Weight::Exact(1),
    ];
    let result = layout(rectangle(0, 0, 1, 1), &input)?;
    let indices: Vec<_> = result.skipped.iter().map(|s| s.input_index).collect();
    assert!(indices.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        result
            .skipped
            .iter()
            .any(|s| s.reason == SkipReason::CoordinatePrecision)
    );
    assert!(
        result
            .skipped
            .iter()
            .any(|s| s.reason == SkipReason::ExactZero)
    );
    assert!(
        result
            .skipped
            .iter()
            .any(|s| s.reason == SkipReason::Unknown)
    );
    assert_eq!(result.skipped.len() + result.shapes.len(), input.len());
    Ok(())
}
