use super::*;

// Fifty large files with distinct sizes plus many small ones whose sizes repeat often.
fn entries(smalls: usize) -> Vec<(String, u64)> {
    let mut entries: Vec<_> = (0..50)
        .map(|i| (format!("big{i:02}"), 10_000 + i))
        .collect();
    entries.extend((0..smalls).map(|i| (format!("s{i:07}"), (i % 50) as u64 + 1)));
    entries
}

fn check(smalls: usize, limit: usize) {
    let version = build(&entries(smalls), ResultCompleteness::Complete);
    let model = project(&version, SizeBasis::Logical, limit).unwrap();
    assert!(
        model.shape_count() <= limit,
        "{} shapes",
        model.shape_count()
    );
    assert!(model.detail.groups.len() <= 1);
    let grouped = model.detail.groups.first().map_or(0, |g| g.members.len());
    let drawn = model.shapes.len();
    assert_eq!(
        drawn + grouped + model.detail.omitted_items as usize,
        smalls + 50
    );
    assert!(model.totals.drawn_bytes + model.totals.grouped_bytes <= bytes_of(&model.totals.scope));
}

#[test]
fn ten_thousand_siblings_stay_within_the_shape_limit() {
    check(10_000, 100);
}

#[test]
fn one_hundred_thousand_siblings_stay_within_the_shape_limit() {
    check(100_000, 100);
    check(100_000, 2);
}

#[test]
fn one_million_siblings_stay_within_the_shape_limit() {
    check(1_000_000, 100);
}
