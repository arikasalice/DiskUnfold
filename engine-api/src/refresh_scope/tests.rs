use super::*;

fn checked(target: u8, parts: &[&str], include_descendants: bool) -> CheckedDirectory<u8, String> {
    CheckedDirectory {
        target,
        components: parts.iter().map(|part| (*part).to_owned()).collect(),
        include_descendants,
    }
}

fn directory(parts: &[&str], include_descendants: bool) -> Directory<String> {
    Directory {
        components: parts.iter().map(|part| (*part).to_owned()).collect(),
        include_descendants,
    }
}

#[test]
fn duplicates_merge_flags_and_only_recursive_ancestors_absorb() {
    let input = vec![
        checked(1, &["a", "child"], false),
        checked(1, &["a"], false),
        checked(1, &["a", "child", "leaf"], false),
        checked(1, &["a", "child"], true),
        checked(1, &["ab"], false),
        checked(2, &["a", "child", "leaf"], false),
    ];
    let result = minimize(input.clone(), 256);
    assert_eq!(
        result.get(&1),
        Some(&TargetSelection::Directories(vec![
            directory(&["a"], false),
            directory(&["a", "child"], true),
            directory(&["ab"], false),
        ]))
    );
    assert_eq!(
        result.get(&2),
        Some(&TargetSelection::Directories(vec![directory(
            &["a", "child", "leaf"],
            false
        )]))
    );
    let mut reversed = input;
    reversed.reverse();
    assert_eq!(result, minimize(reversed.clone(), 256));
    reversed.rotate_left(2);
    assert_eq!(result, minimize(reversed, 256));
}

#[test]
fn component_boundaries_and_empty_root_are_preserved() {
    let result = minimize(
        vec![
            checked(1, &["a"], true),
            checked(1, &["ab", "x"], false),
            checked(1, &["a", "x"], false),
            checked(2, &[], false),
            checked(2, &["x"], false),
            checked(3, &[], true),
            checked(3, &["x"], false),
        ],
        256,
    );
    assert_eq!(
        result.get(&1),
        Some(&TargetSelection::Directories(vec![
            directory(&["a"], true),
            directory(&["ab", "x"], false)
        ]))
    );
    assert_eq!(
        result.get(&2),
        Some(&TargetSelection::Directories(vec![
            directory(&[], false),
            directory(&["x"], false)
        ]))
    );
    assert_eq!(
        result.get(&3),
        Some(&TargetSelection::Directories(vec![directory(&[], true)]))
    );
    assert!(minimize::<u8, String>(vec![], 256).is_empty());
}

#[test]
fn promotion_uses_minimal_count_per_target_and_injected_limit() {
    let mut input = Vec::new();
    for n in 0..257 {
        input.push(CheckedDirectory {
            target: 1,
            components: vec![format!("{n:03}")],
            include_descendants: false,
        });
        if n < 256 {
            input.push(CheckedDirectory {
                target: 2,
                components: vec![format!("{n:03}")],
                include_descendants: false,
            });
        }
    }
    // Repeated rows do not count toward the threshold.
    input.push(checked(2, &["000"], true));
    let result = minimize(input, 256);
    assert_eq!(result.get(&1), Some(&TargetSelection::WholeTarget));
    assert!(
        matches!(result.get(&2), Some(TargetSelection::Directories(rows)) if rows.len() == 256)
    );
    let small = vec![
        checked(1, &["a"], false),
        checked(1, &["b"], false),
        checked(2, &["a"], false),
    ];
    let result = minimize(small.clone(), 1);
    assert_eq!(result.get(&1), Some(&TargetSelection::WholeTarget));
    assert!(matches!(result.get(&2), Some(TargetSelection::Directories(rows)) if rows.len() == 1));
    assert_eq!(
        minimize(small, 0).get(&2),
        Some(&TargetSelection::WholeTarget)
    );
    let mut absorbed = vec![checked(4, &["parent"], true)];
    for n in 0..257 {
        absorbed.push(CheckedDirectory {
            target: 4,
            components: vec!["parent".to_owned(), format!("{n:03}")],
            include_descendants: false,
        });
    }
    assert_eq!(
        minimize(absorbed, 1).get(&4),
        Some(&TargetSelection::Directories(vec![directory(
            &["parent"],
            true
        )]))
    );
}
