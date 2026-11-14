mod group_support;

use std::sync::Arc;

use du_analysis_index::{
    IngestSink, ResultVersion,
    group::{Decision, Error, NodeDecision},
    query::Condition,
};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultVersionRead},
};
use group_support::{
    FileSpec, absent, add_file, build, classify, dataset, dataset_rules, ends_with, err, file_key,
    folder_ref, id, item_key, item_ref, not, parallel, registry, size_at_least, starts_with,
};
fn item_named(version: &ResultVersion, name: &str) -> Result<ItemRef, String> {
    version
        .items()
        .iter()
        .find(|item| item.name == name.as_bytes())
        .map(|item| item_ref(version, item.item_no))
        .ok_or_else(|| format!("missing {name}"))
}

/// A text file with a size, a binary with a size and a text file whose size is unreadable.
fn explain_fixture() -> Result<Arc<ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(err)?;
    build.add_directory(vec![]).map_err(err)?;
    add_file(&mut build, 0, &["doc.txt"], Some(500), 500, 1)?;
    add_file(&mut build, 0, &["pic.bin"], Some(50), 50, 2)?;
    add_file(&mut build, 0, &["note.txt"], None, 5, 3)?;
    build.seal(id(2), ResultCompleteness::Complete).map_err(err)
}

fn node(path: &[u32], predicate: Option<(&str, u32)>, decision: Decision) -> NodeDecision {
    NodeDecision {
        path: path.to_vec(),
        predicate: predicate.map(|(name, revision)| (name.to_owned(), revision)),
        decision,
    }
}

#[test]
fn explanation_records_each_node_and_unknown_never_becomes_a_mismatch() -> Result<(), String> {
    let version = explain_fixture()?;
    let rules = vec![
        // 0: (text or large) and not a picture
        Condition::All(vec![
            Condition::Any(vec![ends_with(".txt"), size_at_least(100)]),
            not(starts_with("pic")),
        ]),
        // 1: a binary or large; unknown when the size is unreadable and the name differs
        Condition::Any(vec![ends_with(".bin"), size_at_least(100)]),
        // 2: negation of an unknown stays unknown
        not(size_at_least(100)),
        // 3: a decisive mismatch settles the group although the other child is unknown
        Condition::All(vec![size_at_least(100), starts_with("zzz")]),
    ];
    let read = classify(&version, None, rules, parallel(1, 1)?)?;
    let registry = registry()?;
    let name = ("pred.name", 1);
    let size = ("pred.size", 1);

    let doc = read
        .explain(&item_named(&version, "doc.txt")?, &registry)
        .map_err(err)?;
    assert_eq!(
        doc[0].nodes,
        [
            node(&[], None, Decision::Match),
            node(&[0], None, Decision::Match),
            node(&[0, 0], Some(name), Decision::Match),
            node(&[0, 1], Some(size), Decision::Match),
            node(&[1], None, Decision::Match),
            node(&[1, 0], Some(name), Decision::NoMatch),
        ]
    );
    assert_eq!(doc[0].decision, Decision::Match);

    let note = read
        .explain(&item_named(&version, "note.txt")?, &registry)
        .map_err(err)?;
    let decisions: Vec<_> = note.iter().map(|rule| rule.decision).collect();
    assert_eq!(
        decisions,
        [
            Decision::Match,
            Decision::Unknown,
            Decision::Unknown,
            Decision::NoMatch
        ]
    );
    assert_eq!(
        note[1].nodes,
        [
            node(&[], None, Decision::Unknown),
            node(&[0], Some(name), Decision::NoMatch),
            node(&[1], Some(size), Decision::Unknown),
        ]
    );
    assert_eq!(note[2].nodes[1].decision, Decision::Unknown);
    assert_eq!(note[3].nodes[1], node(&[0], Some(size), Decision::Unknown));
    assert_eq!(note[0].nodes[3].decision, Decision::Unknown);

    let pic = read
        .explain(&item_named(&version, "pic.bin")?, &registry)
        .map_err(err)?;
    assert_eq!(pic[0].decision, Decision::NoMatch);
    assert_eq!(pic[1].decision, Decision::Match);
    assert_eq!(pic[2].decision, Decision::Match);
    Ok(())
}

#[test]
fn explanation_equals_the_stored_decision_of_every_file_and_rule() -> Result<(), String> {
    let files = dataset(3);
    let version = build(&files, 3)?;
    let mut rules = dataset_rules();
    rules.push(Condition::All(vec![
        starts_with("alpha"),
        size_at_least(100),
    ]));
    rules.push(Condition::Any(vec![absent(), ends_with(".log")]));
    let read = classify(&version, None, rules, parallel(10, 3)?)?;
    let registry = registry()?;
    let mut seen = 0;
    for (item, stored) in read.rows() {
        let explained = read.explain(&item, &registry).map_err(err)?;
        let decisions: Vec<_> = explained.iter().map(|rule| rule.decision).collect();
        assert_eq!(decisions, stored, "{}", item_key(&version, item.item_no));
        assert!(explained.iter().all(|rule| !rule.nodes.is_empty()));
        seen += 1;
    }
    assert_eq!(seen, files.len());
    Ok(())
}

#[test]
fn a_rule_with_an_unregistered_predicate_is_unavailable_at_every_node_that_depends_on_it()
-> Result<(), String> {
    let version = explain_fixture()?;
    let rule = Condition::All(vec![absent(), ends_with(".txt")]);
    let read = classify(&version, None, vec![rule], parallel(1, 1)?)?;
    let doc = read
        .explain(&item_named(&version, "doc.txt")?, &registry()?)
        .map_err(err)?;
    assert_eq!(doc[0].decision, Decision::RuleUnavailable);
    assert_eq!(
        doc[0].nodes,
        [
            node(&[], None, Decision::RuleUnavailable),
            node(&[0], Some(("pred.absent", 1)), Decision::RuleUnavailable),
            node(&[1], Some(("pred.name", 1)), Decision::Match),
        ]
    );
    assert_eq!(read.rule_unavailable(), [true]);
    Ok(())
}

#[test]
fn only_files_of_the_classified_scope_and_version_can_be_explained() -> Result<(), String> {
    let files = dataset(1);
    let version = build(&files, 1)?;
    let docs = folder_ref(&version, 0, "docs")?;
    let rules = vec![ends_with(".txt")];
    let scoped = classify(&version, Some(&docs), rules.clone(), parallel(4, 2)?)?;
    let whole = classify(&version, None, rules, parallel(4, 2)?)?;
    let registry = registry()?;
    let inside = files.iter().find(|f| f.dir == "docs").ok_or("docs file")?;
    let outside = files
        .iter()
        .find(|f| f.dir == "media")
        .ok_or("media file")?;
    let find = |file: &FileSpec| {
        version
            .items()
            .iter()
            .find(|item| item_key(&version, item.item_no) == file_key(file))
            .map(|item| item_ref(&version, item.item_no))
            .ok_or("item")
    };
    assert!(scoped.explain(&find(inside)?, &registry).is_ok());
    assert!(whole.explain(&find(outside)?, &registry).is_ok());
    assert_eq!(
        scoped.explain(&find(outside)?, &registry).map(|_| ()),
        Err(Error::InvalidItem)
    );
    for reference in [
        docs.clone(),
        ItemRef {
            result_version: id(77),
            ..find(inside)?
        },
        ItemRef {
            item_no: u32::MAX,
            ..find(inside)?
        },
    ] {
        assert_eq!(
            scoped.explain(&reference, &registry).map(|_| ()),
            Err(Error::InvalidItem)
        );
        assert_eq!(
            whole.explain(&reference, &registry).map(|_| ()),
            Err(Error::InvalidItem)
        );
    }
    Ok(())
}
