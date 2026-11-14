#![allow(dead_code)]
//! Fixtures shared by the F36 classification tests: result builders, a deterministic
//! dataset with hard-link families and unreadable sizes, and cancellation controls.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use du_analysis_index::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation,
    group::{Classification, Control, Parallelism},
    query::{
        Basis, Boundary, Condition, NameEncoding, NameTest, Predicate, Record, Registry, Sort,
        Truth,
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

pub fn id(n: u32) -> Id {
    let mut bytes = [7u8; 16];
    bytes[..4].copy_from_slice(&n.to_be_bytes());
    Id::from_v7_bytes(bytes)
}

pub fn err<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
}

pub fn size(basis: SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
        evidence: SizeEvidence::OsReported,
    }
}

/// The exact byte count of a total, or `None` when the amount is not exact.
pub fn exact(value: &SizeValue) -> Option<u64> {
    match &value.amount {
        SizeAmount::Exact { bytes } => bytes.as_str().parse().ok(),
        SizeAmount::LowerBound { .. } | SizeAmount::Unknown => None,
    }
}

pub fn bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// Adds a file with its logical size (if readable), allocated size and OS file identity.
pub fn add_file(
    build: &mut BuildHandle,
    root: u32,
    path: &[&str],
    logical: Option<u64>,
    allocated: u64,
    identity: u16,
) -> Result<(), String> {
    let observation = Observation {
        path: path.iter().map(|part| bytes(part)).collect(),
        size: logical.map(|value| size(SizeBasis::Logical, value)),
    };
    let space = SpaceObservation {
        allocated: Some(size(SizeBasis::Allocated, allocated)),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: identity.to_be_bytes().to_vec(),
        }),
        link_count: Some(2),
        sharing: SharingEvidence::None,
    };
    build
        .add_with_space_at(root, observation, space)
        .map_err(err)
}

/// One file of the generated dataset.
#[derive(Clone, Debug)]
pub struct FileSpec {
    pub root: u32,
    pub dir: &'static str,
    pub name: String,
    pub logical: Option<u64>,
    pub allocated: u64,
    pub identity: u16,
}

pub const DIRS: [&str; 4] = ["docs", "media", "src", "tmp"];

/// Builds 56 files in four folders. Every fourth file belongs to one of five hard-link
/// families (same identity and size in different folders), every eleventh file from the
/// fourth has no readable logical size, and the rest are unique with sizes from 1 to 300.
pub fn dataset(roots: u32) -> Vec<FileSpec> {
    let stems = ["alpha", "beta", "gamma", "delta"];
    let extensions = [".log", ".txt", ".bin"];
    (0u32..56)
        .map(|g| {
            let (logical, allocated, identity) = if g % 4 == 0 {
                let family = (g / 4) % 5;
                let bytes = 20 * (u64::from(family) + 1);
                (Some(bytes), bytes, 1000 + family as u16)
            } else if g % 11 == 3 {
                (None, 7 + u64::from(g), 2000 + g as u16)
            } else {
                let bytes = u64::from(g * 37 % 300 + 1);
                (Some(bytes), bytes, 2000 + g as u16)
            };
            FileSpec {
                root: g / 2 % roots,
                dir: DIRS[(g as usize / 3) % 4],
                name: format!(
                    "{}{g}{}",
                    stems[(g as usize / 3) % 4],
                    extensions[g as usize % 3]
                ),
                logical,
                allocated,
                identity,
            }
        })
        .collect()
}

fn scope_root(index: u32) -> ResultScopeRoot {
    let path: Vec<u8> = format!("C:\\Root{index}")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&path),
        },
        volume_id: vec![1],
        file_id: vec![index as u8 + 1],
    }
}

/// Builds one result from the specs: a single root for `roots == 1`, otherwise a forest.
pub fn build(files: &[FileSpec], roots: u32) -> Result<Arc<ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = if roots == 1 {
        sink.open(id(1)).map_err(err)?
    } else {
        sink.open_forest(id(1), (0..roots).map(scope_root).collect())
            .map_err(err)?
    };
    for root in 0..roots {
        build.add_directory_at(root, vec![]).map_err(err)?;
        for dir in DIRS {
            build
                .add_directory_at(root, vec![bytes(dir)])
                .map_err(err)?;
        }
    }
    for file in files {
        let path = [file.dir, file.name.as_str()];
        add_file(
            &mut build,
            file.root,
            &path,
            file.logical,
            file.allocated,
            file.identity,
        )?;
    }
    build.seal(id(2), ResultCompleteness::Complete).map_err(err)
}

/// The slash-separated key of an item: `r<root>:<folder>/<name>`.
pub fn item_key(version: &ResultVersion, item_no: u32) -> String {
    let items = version.items();
    let mut parts = Vec::new();
    let mut current = items.get(item_no as usize);
    while let Some(item) = current {
        match item.parent {
            Some(parent) => {
                parts.push(String::from_utf8_lossy(&item.name).into_owned());
                current = items.get(parent as usize);
            }
            None => {
                parts.push(format!("r{}:", item.item_no));
                break;
            }
        }
    }
    parts.reverse();
    parts.join("/").replacen(":/", ":", 1)
}

pub fn file_key(file: &FileSpec) -> String {
    format!("r{}:{}/{}", file.root, file.dir, file.name)
}

/// The reference version of a root-relative item, used as a scope.
pub fn item_ref(version: &ResultVersion, item_no: u32) -> ItemRef {
    ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    }
}

pub fn folder_ref(version: &ResultVersion, root: u32, dir: &str) -> Result<ItemRef, String> {
    let top = version
        .items()
        .get(root as usize)
        .ok_or("missing root")?
        .item_no;
    version
        .items()
        .iter()
        .find(|item| item.parent == Some(top) && item.name == dir.as_bytes())
        .map(|item| item_ref(version, item.item_no))
        .ok_or_else(|| format!("missing folder {dir}"))
}

pub fn name_rule(test: NameTest) -> Condition {
    Condition::Predicate(Predicate::Name(test, NameEncoding::Bytes))
}

pub fn ends_with(suffix: &str) -> Condition {
    name_rule(NameTest::EndsWith(bytes(suffix)))
}

pub fn starts_with(prefix: &str) -> Condition {
    name_rule(NameTest::StartsWith(bytes(prefix)))
}

pub fn registered(id: &str) -> Condition {
    Condition::Predicate(Predicate::Registered {
        id: id.into(),
        revision: 1,
        operand: vec![],
    })
}

/// A rule whose predicate is absent from every registry.
pub fn absent() -> Condition {
    registered("pred.absent")
}

fn always_unknown(_: &Record<'_>, _: &Predicate) -> Truth {
    Truth::Unknown
}

/// The baseline registry plus `pred.unknown`, which never decides.
pub fn registry() -> Result<Registry, String> {
    let mut registry = Registry::baseline();
    registry
        .register_predicate("pred.unknown", 1, always_unknown)
        .map_err(err)?;
    Ok(registry)
}

pub fn parallel(chunk_items: usize, workers: usize) -> Result<Parallelism, String> {
    Ok(Parallelism {
        chunk_items: std::num::NonZeroUsize::new(chunk_items).ok_or("chunk")?,
        workers: std::num::NonZeroUsize::new(workers).ok_or("workers")?,
    })
}

/// A control that never cancels and records every progress report.
#[derive(Default)]
pub struct Recording {
    cancel_from_poll: Option<usize>,
    polls: AtomicUsize,
    reports: Mutex<Vec<u64>>,
}

impl Recording {
    /// Cancels at the given one-based poll and at every later poll.
    pub fn cancel_from(poll: usize) -> Self {
        Self {
            cancel_from_poll: Some(poll),
            ..Self::default()
        }
    }

    pub fn polls(&self) -> usize {
        self.polls.load(Ordering::SeqCst)
    }

    pub fn reports(&self) -> Vec<u64> {
        self.reports.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

impl Control for Recording {
    fn cancelled(&self) -> bool {
        let poll = self.polls.fetch_add(1, Ordering::SeqCst) + 1;
        self.cancel_from_poll.is_some_and(|from| poll >= from)
    }

    fn progress(&self, completed_files: u64) {
        if let Ok(mut reports) = self.reports.lock() {
            reports.push(completed_files);
        }
    }
}

/// Classifies with the default registry extended by `pred.unknown`.
pub fn classify(
    version: &Arc<ResultVersion>,
    root: Option<&ItemRef>,
    rules: Vec<Condition>,
    parallelism: Parallelism,
) -> Result<Classification, String> {
    du_analysis_index::group::classify_scope(
        Arc::clone(version),
        root,
        &registry()?,
        rules,
        parallelism,
        &Recording::default(),
    )
    .map_err(err)
}

/// A folder with a hard-link pair and one other file, a second folder with one file and an
/// empty folder. Returns the result and the reference of the first folder.
pub fn alias_fixture() -> Result<(Arc<ResultVersion>, ItemRef), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(err)?;
    for folder in [&[][..], &["scope"], &["outside"], &["empty"]] {
        build
            .add_directory(folder.iter().map(|part| bytes(part)).collect())
            .map_err(err)?;
    }
    add_file(&mut build, 0, &["scope", "a-alias"], Some(8), 8, 1)?;
    add_file(&mut build, 0, &["scope", "b-alias"], Some(8), 8, 1)?;
    add_file(&mut build, 0, &["scope", "z-other"], Some(4), 4, 2)?;
    add_file(&mut build, 0, &["outside", "a-alias"], Some(64), 64, 3)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let scope = folder_ref(&version, 0, "scope")?;
    Ok((version, scope))
}

pub fn sort(id: &'static str, descending: bool) -> Sort<'static> {
    Sort {
        id,
        revision: 1,
        basis: Basis::Logical,
        descending,
    }
}

pub fn size_at_least(bytes: u64) -> Condition {
    Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtLeast(bytes)))
}

pub fn not(condition: Condition) -> Condition {
    Condition::Not(Box::new(condition))
}

pub fn dataset_rules() -> Vec<Condition> {
    vec![
        ends_with(".txt"),
        not(ends_with(".bin")),
        absent(),
        registered("pred.unknown"),
    ]
}
