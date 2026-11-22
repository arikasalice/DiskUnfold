use super::*;
use du_core_contracts::result::ResultItemKind;

fn observation(kind: ResultItemKind) -> StoredObservation {
    StoredObservation {
        size: None,
        kind,
        modified_at: None,
        native_modified_at: None,
        enumerated_inode: None,
        placeholder_observation: None,
        observed_at: None,
    }
}

fn path(names: &[&str]) -> Vec<Vec<u8>> {
    names.iter().map(|name| name.as_bytes().to_vec()).collect()
}

fn add(store: &mut ObservationStore, root: u32, names: &[&str]) -> Result<(), InsertError> {
    store.insert(root, &path(names), observation(ResultItemKind::Directory))
}

#[test]
fn a_full_store_rejects_another_node() {
    let mut store = ObservationStore::with_limit(3);
    assert_eq!(add(&mut store, 0, &[]), Ok(()));
    assert_eq!(add(&mut store, 0, &["a"]), Ok(()));
    assert_eq!(add(&mut store, 0, &["b"]), Ok(()));
    assert_eq!(add(&mut store, 0, &["c"]), Err(InsertError::Capacity));
    // An ancestor created for a deeper path counts against the same limit.
    let mut store = ObservationStore::with_limit(2);
    assert_eq!(add(&mut store, 0, &["a", "b"]), Err(InsertError::Capacity));
    assert_eq!(store.nodes(), 2);
}

#[test]
fn a_second_observation_of_one_path_is_a_duplicate() {
    let mut store = ObservationStore::default();
    assert_eq!(add(&mut store, 0, &[]), Ok(()));
    assert_eq!(add(&mut store, 0, &[]), Err(InsertError::Duplicate));
    assert_eq!(add(&mut store, 0, &["a", "b"]), Ok(()));
    assert_eq!(add(&mut store, 0, &["a", "b"]), Err(InsertError::Duplicate));
    // The ancestor named by the deeper path is not observed until it is stored itself.
    assert!(store.has_unobserved());
    assert_eq!(add(&mut store, 0, &["a"]), Ok(()));
    assert!(!store.has_unobserved());
    assert_eq!(add(&mut store, 0, &["a"]), Err(InsertError::Duplicate));
}

#[test]
fn the_index_keeps_every_child_through_growth() -> Result<(), InsertError> {
    let mut store = ObservationStore::default();
    add(&mut store, 0, &[])?;
    let names: Vec<String> = (0..5_000).map(|n| format!("n{n}")).collect();
    for name in &names {
        add(&mut store, 0, &[name])?;
        add(&mut store, 0, &[name, "leaf"])?;
    }
    assert_eq!(store.nodes(), 10_001);
    for name in &names {
        let directory = store.find(0, &path(&[name]));
        let leaf = store.find(0, &path(&[name, "leaf"]));
        assert!(directory.is_some() && leaf.is_some());
        assert_eq!(leaf.and_then(|node| store.parent(node)), directory);
        assert_eq!(leaf.map(|node| store.name(node)), Some(&b"leaf"[..]));
    }
    assert_eq!(store.find(0, &path(&["n5000"])), None);
    assert_eq!(store.find(0, &path(&["leaf"])), None);
    Ok(())
}

#[test]
fn equal_names_below_different_parents_and_roots_stay_apart() -> Result<(), InsertError> {
    let mut store = ObservationStore::default();
    for root in [0, 1] {
        add(&mut store, root, &[])?;
        for names in [&["a"][..], &["b"], &["a", "x"], &["b", "x"], &["x"]] {
            add(&mut store, root, names)?;
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for root in [0, 1] {
        for names in [&["a", "x"][..], &["b", "x"], &["x"]] {
            let expected = store.find(root, &path(names));
            // The remembered node of the previous lookup has the same last name.
            assert_eq!(store.locate(root, &path(names)), expected);
            assert!(expected.is_some_and(|node| seen.insert(node)));
        }
    }
    assert_eq!(store.locate(0, &path(&["a", "x", "x"])), None);
    assert_eq!(store.locate(2, &path(&["x"])), None);
    assert_eq!(store.locate(1, &[]), store.root_node(1));
    Ok(())
}

#[test]
fn interleaved_directories_resolve_like_a_walk_from_the_root() -> Result<(), InsertError> {
    let mut store = ObservationStore::default();
    add(&mut store, 0, &[])?;
    let directories: Vec<String> = (0..40).map(|n| format!("d{n}")).collect();
    for name in &directories {
        add(&mut store, 0, &[name])?;
        add(&mut store, 0, &[name, "same"])?;
    }
    // More directories than the lookup memory holds, visited in turn.
    for file in 0..3 {
        for name in &directories {
            let leaf = format!("f{file}");
            store.insert(
                0,
                &path(&[name, "same", &leaf]),
                observation(ResultItemKind::File),
            )?;
            let names = path(&[name, "same", &leaf]);
            let found = store.find(0, &names);
            assert_eq!(store.locate(0, &names), found);
            assert_eq!(
                found.and_then(|node| store.parent(node)),
                store.find(0, &path(&[name, "same"]))
            );
        }
    }
    assert!(!store.has_unobserved());
    Ok(())
}

#[test]
fn the_deepest_node_stops_at_the_first_missing_name() -> Result<(), InsertError> {
    let mut store = ObservationStore::default();
    add(&mut store, 0, &[])?;
    add(&mut store, 0, &["a"])?;
    let root = store.root_node(0);
    let a = store.find(0, &path(&["a"]));
    assert_eq!(store.deepest(0, &[]), root.map(|node| (node, true)));
    assert_eq!(store.deepest(0, &path(&["a"])), a.map(|node| (node, true)));
    assert_eq!(
        store.deepest(0, &path(&["a", "b", "c"])),
        a.map(|node| (node, false))
    );
    assert_eq!(
        store.deepest(0, &path(&["z"])),
        root.map(|node| (node, false))
    );
    assert_eq!(store.deepest(1, &[]), None);
    Ok(())
}

#[test]
fn an_observation_is_changed_only_through_its_exact_path() -> Result<(), InsertError> {
    let mut store = ObservationStore::default();
    add(&mut store, 0, &["a", "b"])?;
    // The ancestors exist as nodes but hold no observation.
    assert!(store.get_mut(0, &path(&["a"])).is_none());
    assert!(store.get_mut(0, &[]).is_none());
    assert!(store.get_mut(0, &path(&["a", "b", "c"])).is_none());
    let stored = store.get_mut(0, &path(&["a", "b"]));
    assert_eq!(
        stored.map(|value| value.kind),
        Some(ResultItemKind::Directory)
    );
    assert!(!store.is_empty());
    Ok(())
}
