mod group_support;

use std::collections::{BTreeMap, BTreeSet};

use du_analysis_index::{
    IngestSink,
    group::{self, Category, Classification, Parallelism},
    query::{Basis, Boundary, Condition, Predicate},
};
use du_core_contracts::{SizeBasis, result::ResultCompleteness};
use group_support::{
    FileSpec, absent, add_file, build, classify, dataset, ends_with, err, exact, file_key,
    folder_ref, id, item_key, parallel, registered, registry, size, starts_with,
};

/// A rule as the test understands it. Its judgement is written without the engine's
/// evaluator, from the facts of each generated file.
#[derive(Clone)]
enum Spec {
    EndsWith(&'static str),
    StartsWith(&'static str),
    SizeAtLeast(u64),
    Not(Box<Spec>),
    All(Vec<Spec>),
    Any(Vec<Spec>),
    Unknown,
    Absent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tri {
    Yes,
    No,
    Maybe,
}

impl Spec {
    fn condition(&self) -> Condition {
        match self {
            Self::EndsWith(suffix) => ends_with(suffix),
            Self::StartsWith(prefix) => starts_with(prefix),
            Self::SizeAtLeast(n) => {
                Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtLeast(*n)))
            }
            Self::Not(inner) => Condition::Not(Box::new(inner.condition())),
            Self::All(list) => Condition::All(list.iter().map(Self::condition).collect()),
            Self::Any(list) => Condition::Any(list.iter().map(Self::condition).collect()),
            Self::Unknown => registered("pred.unknown"),
            Self::Absent => absent(),
        }
    }

    /// `None` means the rule uses a predicate that no registry holds.
    fn judge(&self, file: &FileSpec) -> Option<Tri> {
        let yes_no = |yes: bool| Some(if yes { Tri::Yes } else { Tri::No });
        match self {
            Self::EndsWith(suffix) => yes_no(file.name.ends_with(suffix)),
            Self::StartsWith(prefix) => yes_no(file.name.starts_with(prefix)),
            Self::SizeAtLeast(n) => file.logical.map_or(Some(Tri::Maybe), |v| yes_no(v >= *n)),
            Self::Not(inner) => inner.judge(file).map(|t| match t {
                Tri::Yes => Tri::No,
                Tri::No => Tri::Yes,
                Tri::Maybe => Tri::Maybe,
            }),
            Self::All(list) => combine(list, file, Tri::No, Tri::Yes),
            Self::Any(list) => combine(list, file, Tri::Yes, Tri::No),
            Self::Unknown => Some(Tri::Maybe),
            Self::Absent => None,
        }
    }
}

/// A decisive child settles the group; otherwise any uncertain child leaves it uncertain.
fn combine(list: &[Spec], file: &FileSpec, decisive: Tri, neutral: Tri) -> Option<Tri> {
    let verdicts: Option<Vec<Tri>> = list.iter().map(|spec| spec.judge(file)).collect();
    let verdicts = verdicts?;
    Some(if verdicts.contains(&decisive) {
        decisive
    } else if verdicts.contains(&Tri::Maybe) {
        Tri::Maybe
    } else {
        neutral
    })
}

/// The reference membership of one file.
struct Reference {
    file: FileSpec,
    matched: Vec<usize>,
    undecided: Vec<usize>,
    has_unknown: bool,
    has_unavailable: bool,
}

fn reference(files: &[FileSpec], specs: &[Spec]) -> Vec<Reference> {
    let mut ordered = files.to_vec();
    ordered.sort_by(|a, b| (a.root, a.dir, &a.name).cmp(&(b.root, b.dir, &b.name)));
    ordered
        .into_iter()
        .map(|file| {
            let verdicts: Vec<_> = specs.iter().map(|spec| spec.judge(&file)).collect();
            Reference {
                matched: (0..verdicts.len())
                    .filter(|&i| verdicts[i] == Some(Tri::Yes))
                    .collect(),
                undecided: (0..verdicts.len())
                    .filter(|&i| matches!(verdicts[i], None | Some(Tri::Maybe)))
                    .collect(),
                has_unknown: verdicts.contains(&Some(Tri::Maybe)),
                has_unavailable: verdicts.contains(&None),
                file,
            }
        })
        .collect()
}

/// Files whose reference membership satisfies `keep`, in observed path order.
fn members(reference: &[Reference], keep: impl Fn(&Reference) -> bool) -> Vec<&FileSpec> {
    reference
        .iter()
        .filter(|r| keep(r))
        .map(|r| &r.file)
        .collect()
}

fn read_keys(read: &Classification, category: Category) -> Result<Vec<String>, String> {
    let page = read
        .page(category, &[], &registry()?, 0, usize::MAX, || false)
        .map_err(err)?;
    assert_eq!(page.total, page.rows.len());
    Ok(page
        .rows
        .iter()
        .map(|row| item_key(read.version(), row.item.item_no))
        .collect())
}

fn keys(files: &[&FileSpec]) -> Vec<String> {
    files.iter().map(|file| file_key(file)).collect()
}

/// The totals F03 must report for a set of files, computed from raw numbers.
/// Logical sizes add per file; space on disk adds once per file identity.
fn raw(files: &[&FileSpec]) -> (Option<u64>, Option<u64>) {
    let logical = files.iter().map(|f| f.logical).sum::<Option<u64>>();
    let identities: BTreeMap<u16, u64> = files.iter().map(|f| (f.identity, f.allocated)).collect();
    (logical, Some(identities.values().sum()))
}

fn assert_total(
    label: &str,
    total: &du_analysis_index::group::Totals,
    files: &[&FileSpec],
    field: fn(&du_analysis_index::group::Totals) -> &du_core_contracts::result::SpaceAggregate,
) {
    let (logical, allocated) = raw(files);
    let value = field(total);
    assert_eq!(exact(&value.logical), logical, "{label} logical");
    assert_eq!(exact(&value.allocated), allocated, "{label} allocated");
    assert_eq!(
        exact(&value.unique_physical),
        allocated,
        "{label} unique physical"
    );
}

fn rule_sets() -> Vec<Vec<Spec>> {
    vec![
        vec![
            Spec::EndsWith(".txt"),
            Spec::StartsWith("alpha"),
            Spec::SizeAtLeast(150),
            Spec::Not(Box::new(Spec::EndsWith(".bin"))),
            Spec::All(vec![Spec::EndsWith(".log"), Spec::SizeAtLeast(100)]),
            Spec::Any(vec![Spec::StartsWith("gamma"), Spec::SizeAtLeast(280)]),
        ],
        vec![
            Spec::EndsWith(".txt"),
            Spec::StartsWith("beta"),
            Spec::Absent,
        ],
        vec![],
        vec![Spec::SizeAtLeast(250), Spec::Unknown],
        vec![Spec::All(vec![Spec::EndsWith(".txt"), Spec::Absent])],
    ]
}

/// The reference categories of one rule set, each in observed path order.
struct Sets<'a> {
    all: Vec<&'a FileSpec>,
    groups: Vec<Vec<&'a FileSpec>>,
    overlap: Vec<&'a FileSpec>,
    union: Vec<&'a FileSpec>,
    unclassified: Vec<&'a FileSpec>,
    undecidable: Vec<&'a FileSpec>,
}

fn sets(expected: &[Reference], rules: usize) -> Sets<'_> {
    Sets {
        all: members(expected, |_| true),
        groups: (0..rules)
            .map(|g| members(expected, |r| r.matched.contains(&g)))
            .collect(),
        overlap: members(expected, |r| r.matched.len() >= 2),
        union: members(expected, |r| !r.matched.is_empty()),
        unclassified: members(expected, |r| r.matched.is_empty() && r.undecided.is_empty()),
        undecidable: members(expected, |r| {
            r.matched.is_empty() && !r.undecided.is_empty()
        }),
    }
}

fn check_members(read: &Classification, sets: &Sets<'_>) -> Result<(), String> {
    for (g, group) in sets.groups.iter().enumerate() {
        assert_eq!(
            read_keys(read, Category::Group(g))?,
            keys(group),
            "group {g}"
        );
    }
    assert_eq!(read_keys(read, Category::Overlap)?, keys(&sets.overlap));
    assert_eq!(
        read_keys(read, Category::Unclassified)?,
        keys(&sets.unclassified)
    );
    assert_eq!(
        read_keys(read, Category::Undecidable)?,
        keys(&sets.undecidable)
    );
    Ok(())
}

fn check_summary(read: &Classification, expected: &[Reference], sets: &Sets<'_>) {
    let summary = read.summary();
    assert_eq!(summary.scope_files as usize, sets.all.len());
    assert_eq!(summary.matched as usize, sets.union.len());
    assert_eq!(summary.overlap as usize, sets.overlap.len());
    assert_eq!(summary.unclassified as usize, sets.unclassified.len());
    assert_eq!(summary.undecidable as usize, sets.undecidable.len());
    assert!(summary.reconciled);
    assert_eq!(
        summary.matched + summary.unclassified + summary.undecidable,
        summary.scope_files
    );
    for (g, counts) in summary.groups.iter().enumerate() {
        let count = |keep: &dyn Fn(&Reference) -> bool| expected.iter().filter(|r| keep(r)).count();
        assert_eq!(counts.members as usize, sets.groups[g].len());
        assert_eq!(
            counts.overlap_members as usize,
            count(&|r| r.matched.contains(&g) && r.matched.len() >= 2)
        );
        assert_eq!(
            counts.undecidable as usize,
            count(&|r| r.undecided.contains(&g))
        );
    }
    let reason = |key| summary.reason_counts.get(key).map_or(0, |n| *n as usize);
    let undecidable = |keep: fn(&Reference) -> bool| {
        expected
            .iter()
            .filter(|r| r.matched.is_empty() && keep(r))
            .count()
    };
    assert_eq!(
        reason(group::REASON_VALUE_UNKNOWN),
        undecidable(|r| r.has_unknown)
    );
    assert_eq!(
        reason(group::REASON_RULE_UNEVALUABLE),
        undecidable(|r| r.has_unavailable)
    );
}

/// Integrated totals for every selection: all rules, then a subset when there are enough.
fn check_totals(
    read: &Classification,
    expected: &[Reference],
    sets: &Sets<'_>,
) -> Result<(), String> {
    let rules = sets.groups.len();
    let selections = if rules > 3 {
        vec![vec![], vec![1, 3]]
    } else {
        vec![vec![]]
    };
    for selected in selections {
        let totals = read.totals(&selected, || false).map_err(err)?;
        let chosen = |g: usize| selected.is_empty() || selected.contains(&g);
        let union = members(expected, |r| r.matched.iter().any(|&g| chosen(g)));
        let overlap = members(expected, |r| {
            r.matched.iter().filter(|&&g| chosen(g)).count() >= 2
        });
        assert_total("union", &totals, &union, |t| &t.union);
        assert_total("overlap", &totals, &overlap, |t| &t.overlap);
        assert_eq!(totals.overlap_count, overlap.len());
        assert_total("unclassified", &totals, &sets.unclassified, |t| {
            &t.unclassified
        });
        assert_total("undecidable", &totals, &sets.undecidable, |t| {
            &t.undecidable
        });
        assert_total("scope", &totals, &sets.all, |t| &t.scope);
        assert_eq!(totals.groups.len(), rules);
        for (g, group) in sets.groups.iter().enumerate() {
            let (logical, allocated) = raw(group);
            assert_eq!(
                exact(&totals.groups[g].logical),
                logical,
                "group {g} logical"
            );
            assert_eq!(
                exact(&totals.groups[g].allocated),
                allocated,
                "group {g} allocated"
            );
        }
    }
    Ok(())
}

fn check(roots: u32, specs: &[Spec], parallelism: Parallelism) -> Result<(), String> {
    let files = dataset(roots);
    let version = build(&files, roots)?;
    let conditions = specs.iter().map(Spec::condition).collect();
    let read = classify(&version, None, conditions, parallelism)?;
    let expected = reference(&files, specs);
    let sets = sets(&expected, specs.len());
    check_members(&read, &sets)?;
    check_summary(&read, &expected, &sets);
    check_totals(&read, &expected, &sets)
}
#[test]
fn single_root_membership_and_totals_match_the_reference_for_every_rule_set() -> Result<(), String>
{
    for specs in rule_sets() {
        check(1, &specs, parallel(5, 3)?)?;
    }
    Ok(())
}

#[test]
fn a_forest_is_classified_across_all_its_roots() -> Result<(), String> {
    for specs in rule_sets() {
        check(3, &specs, parallel(4, 2)?)?;
    }
    let files = dataset(3);
    let version = build(&files, 3)?;
    let read = classify(&version, None, vec![ends_with(".txt")], parallel(100, 1)?)?;
    assert_eq!(read.root(), None);
    assert_eq!(read.summary().scope_files as usize, files.len());
    let roots: BTreeSet<_> = read_keys(&read, Category::Unclassified)?
        .iter()
        .map(|key| key[..2].to_owned())
        .collect();
    assert_eq!(roots, ["r0", "r1", "r2"].map(String::from).into());
    // A folder of one root limits the same classification to that root.
    let docs = folder_ref(&version, 1, "docs")?;
    let narrow = classify(
        &version,
        Some(&docs),
        vec![ends_with(".txt")],
        parallel(100, 1)?,
    )?;
    let expected = files
        .iter()
        .filter(|f| f.root == 1 && f.dir == "docs")
        .count();
    assert_eq!(narrow.summary().scope_files as usize, expected);
    assert_eq!(
        group::scope_file_count(&version, Some(&docs)),
        Ok(expected as u64)
    );
    Ok(())
}

/// Two links to one file land in different groups.
#[test]
fn hard_links_in_two_groups_count_once_in_the_union() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(err)?;
    build.add_directory(vec![]).map_err(err)?;
    add_file(&mut build, 0, &["left"], Some(8), 8, 1)?;
    add_file(&mut build, 0, &["right"], Some(8), 8, 1)?;
    add_file(&mut build, 0, &["other"], Some(4), 4, 2)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let read = classify(
        &version,
        None,
        vec![starts_with("left"), starts_with("right")],
        parallel(1, 1)?,
    )?;
    let totals = read.totals(&[], || false).map_err(err)?;
    let value = |basis, bytes| size(basis, bytes);
    assert_eq!(totals.groups[0].allocated, value(SizeBasis::Allocated, 8));
    assert_eq!(totals.groups[1].allocated, value(SizeBasis::Allocated, 8));
    assert_eq!(totals.union.allocated, value(SizeBasis::Allocated, 8));
    assert_eq!(totals.union.logical, value(SizeBasis::Logical, 16));
    assert_eq!(
        totals.union.unique_physical.amount,
        totals.union.allocated.amount
    );
    assert_eq!(totals.scope.allocated, value(SizeBasis::Allocated, 12));
    assert_eq!(totals.overlap_count, 0);
    assert_eq!(exact(&totals.overlap.allocated), Some(0));
    Ok(())
}
