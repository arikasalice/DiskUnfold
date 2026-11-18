use du_analysis_index::visualization::{
    detail::{Limits, Plan, Tail, min_area, plan, rect_area},
    geometry::Rect,
};

// Area 10_000 and parent 100 bytes: a sibling needs bytes * 10_000 >= min_area * 100.
fn limits(min_area: u64, slots: usize) -> Limits {
    Limits {
        area: 10_000,
        min_area,
        slots,
        allow_group: true,
    }
}

#[test]
fn a_sibling_exactly_at_the_minimum_area_is_retained_and_one_byte_less_is_not() {
    // min_area 1_000 -> threshold is exactly 10 bytes of the 100.
    let kept = plan(&[60, 10, 9, 9, 9, 3], 100, limits(1_000, 10));
    assert_eq!(kept.retained, 2);
    // The tail 9 + 9 + 9 + 3 = 30 reaches the minimum as a group.
    assert_eq!(kept.tail, Tail::Group { bytes: 30 });
    assert_eq!(kept.group_slot, 1);
    assert_eq!(kept.blank, 0);
    let short = plan(&[60, 10, 9], 100, limits(1_000, 10));
    assert_eq!(short.retained, 2);
    // A tail of 9 bytes is below the 10-byte minimum: it is omitted, not grouped.
    assert_eq!(short.tail, Tail::Omitted);
}

#[test]
fn a_tail_below_the_minimum_is_omitted_and_its_bytes_stay_blank() {
    let omitted = plan(&[50, 5, 4], 1_000, limits(100, 10));
    // threshold: bytes * 10_000 >= 100 * 1_000 -> 10 bytes; tail 9 < 10.
    assert_eq!(omitted.retained, 1);
    assert_eq!(omitted.tail, Tail::Omitted);
    assert_eq!(omitted.group_slot, 0);
    assert_eq!(omitted.blank, 1_000 - 50);
}

#[test]
fn every_sibling_kept_leaves_no_tail() {
    let all = plan(&[50, 30, 20], 100, limits(1_000, 10));
    assert_eq!(all.retained, 3);
    assert_eq!(all.tail, Tail::None);
    assert_eq!(all.blank, 0);
    let empty = plan(&[], 100, limits(1_000, 10));
    assert_eq!(
        empty,
        Plan {
            retained: 0,
            tail: Tail::None,
            group_slot: 0,
            blank: 100
        }
    );
}

#[test]
fn the_group_sits_after_siblings_of_its_own_size() {
    // Retained 30 and 20; the tail 10 + 10 = 20 equals the second sibling.
    let planned = plan(&[30, 20, 10, 10], 100, limits(1_500, 10));
    assert_eq!(planned.retained, 2);
    assert_eq!(planned.tail, Tail::Group { bytes: 20 });
    assert_eq!(planned.group_slot, 2);
}

#[test]
fn short_slots_cut_back_to_a_size_boundary_and_reserve_one_for_the_group() {
    // Four equal siblings: a cut at two or three would split a run of equal sizes.
    let bytes = [30, 30, 30, 10];
    for slots in [1, 2, 3] {
        let planned = plan(&bytes, 100, limits(1, slots));
        assert_eq!(planned.retained, 0, "slots {slots}");
        assert_eq!(planned.tail, Tail::Group { bytes: 100 });
    }
    let fits = plan(&bytes, 100, limits(1, 4));
    assert_eq!((fits.retained, fits.tail), (4, Tail::None));
    // One slot is kept for the group: 40 retained, then the 60 byte tail in a group.
    let reserved = plan(&[40, 12, 12, 12, 12, 12], 100, limits(1_500, 1));
    assert_eq!(reserved.retained, 0);
    assert_eq!(reserved.tail, Tail::Group { bytes: 100 });
    let reserved = plan(&[40, 12, 12, 12, 12, 12], 100, limits(1_500, 2));
    assert_eq!(reserved.retained, 1);
    assert_eq!(reserved.tail, Tail::Group { bytes: 60 });
    // No slot at all: nothing is drawn and everything is counted.
    let none = plan(&[40, 12], 100, limits(1, 0));
    assert_eq!(
        (none.retained, none.tail, none.blank),
        (0, Tail::Omitted, 100)
    );
}

#[test]
fn minimum_area_rounds_up_and_a_zero_limit_is_one() {
    assert_eq!(min_area(1_000_000_000_000, 3), 333_333_333_334);
    assert_eq!(min_area(10, 0), 10);
    assert_eq!(min_area(0, 5), 1);
    let window = Rect {
        left: 100,
        top: 200,
        right: 1_100,
        bottom: 700,
    };
    assert_eq!(rect_area(window), 500_000);
}

#[test]
fn extreme_values_do_not_overflow() {
    let huge = u64::MAX / 4;
    let planned = plan(
        &[huge, huge, huge],
        u64::MAX,
        Limits {
            area: 1_000_000_000_000,
            min_area: 1,
            slots: 2,
            allow_group: true,
        },
    );
    // Three equal siblings and two slots: the cut falls back to the boundary.
    assert_eq!(planned.retained, 0);
    assert!(matches!(planned.tail, Tail::Group { .. }));
}

// Deterministic pseudo-random inputs; no dependency is needed for the invariants.
fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

#[test]
fn plans_keep_every_invariant_for_many_inputs() {
    let mut state = 7_u64;
    for _ in 0..4_000 {
        let count = (next(&mut state) % 40) as usize;
        let mut bytes: Vec<u64> = (0..count).map(|_| next(&mut state) % 50 + 1).collect();
        bytes.sort_unstable_by(|a, b| b.cmp(a));
        let sum: u64 = bytes.iter().sum();
        let parent = sum + next(&mut state) % 100;
        let area = 1_000_000 * (next(&mut state) % 9 + 1);
        let shapes = (next(&mut state) % 12) as usize;
        let min = min_area(area, shapes);
        let planned = plan(
            &bytes,
            parent,
            Limits {
                area,
                min_area: min,
                slots: shapes,
                allow_group: true,
            },
        );
        let group = u64::from(matches!(planned.tail, Tail::Group { .. }));
        assert!(
            planned.retained as u64 + group <= shapes as u64,
            "{bytes:?}"
        );
        // The cut never separates equal sizes, so size alone decides each side.
        if planned.retained > 0 && planned.retained < bytes.len() {
            assert_ne!(bytes[planned.retained - 1], bytes[planned.retained]);
        }
        let head: u64 = bytes[..planned.retained].iter().sum();
        let tail: u64 = bytes[planned.retained..].iter().sum();
        let covered = head
            + if let Tail::Group { bytes } = planned.tail {
                bytes
            } else {
                0
            };
        assert_eq!(planned.blank, parent - covered);
        if let Tail::Group { bytes } = planned.tail {
            assert_eq!(bytes, tail);
            assert!(planned.group_slot <= planned.retained);
        }
        // Disjoint shapes that each reach the minimum area fit in the area.
        let drawn = planned.retained as u64 + group;
        assert!(
            u128::from(drawn) * u128::from(min) <= u128::from(area),
            "{bytes:?}"
        );
        // Same input, same plan.
        assert_eq!(
            planned,
            plan(
                &bytes,
                parent,
                Limits {
                    area,
                    min_area: min,
                    slots: shapes,
                    allow_group: true,
                }
            )
        );
    }
}

#[test]
fn a_tail_that_may_not_group_is_omitted_and_its_bytes_stay_blank() {
    let mut closed = limits(1_000, 10);
    closed.allow_group = false;
    let planned = plan(&[60, 10, 9, 9, 9, 3], 100, closed);
    assert_eq!(planned.retained, 2);
    assert_eq!(planned.tail, Tail::Omitted);
    assert_eq!(planned.group_slot, 0);
    assert_eq!(planned.blank, 30);
}
