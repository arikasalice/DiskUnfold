use du_core_contracts::result::ResultItemKind;
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};

/// The outcome of a predicate; unavailable facts are never interpreted as a mismatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Truth {
    Match,
    NoMatch,
    Unknown,
}

/// Literal matching over the native name bytes supplied by the result version.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameTest {
    Equal(Vec<u8>),
    Contains(Vec<u8>),
    StartsWith(Vec<u8>),
    EndsWith(Vec<u8>),
}

/// Native filename encoding supplied by the caller for literal comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameEncoding {
    Bytes,
    WindowsUtf16Le,
}

/// Item kinds that the current result source can identify.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    File,
    Directory,
    Link,
}

/// Supported size bases; physical unique size is not an item property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Basis {
    Logical,
    Allocated,
}

/// Integer byte boundary and inclusive or exclusive comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Boundary {
    AtLeast(u64),
    GreaterThan(u64),
    AtMost(u64),
    LessThan(u64),
}

/// A supported leaf condition and its pinned registry revision.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Predicate {
    Name(NameTest, NameEncoding),
    Kind(Kind),
    Size(Basis, Boundary),
    Registered {
        id: String,
        revision: u32,
        operand: Vec<u8>,
    },
}

impl Predicate {
    /// Returns the registered predicate ID and revision used by this condition.
    pub fn strategy(&self) -> (&str, u32) {
        match self {
            Self::Name(..) => ("pred.name", 1),
            Self::Kind(_) => ("pred.type", 1),
            Self::Size(..) => ("pred.size", 1),
            Self::Registered { id, revision, .. } => (id, *revision),
        }
    }
}

/// Canonical boolean expression. Empty conjunction and disjunction are true and false.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Condition {
    Predicate(Predicate),
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
}

impl Condition {
    /// Flattens like conjunctions or disjunctions and deterministically orders their children.
    pub fn normalize(self) -> Self {
        match self {
            Self::All(children) => Self::All(normalize_children(children, true)),
            Self::Any(children) => Self::Any(normalize_children(children, false)),
            Self::Not(child) => Self::Not(Box::new(child.normalize())),
            leaf => leaf,
        }
    }
}

fn normalize_children(children: Vec<Condition>, all: bool) -> Vec<Condition> {
    let mut result = Vec::new();
    for child in children {
        match child.normalize() {
            Condition::All(nested) if all => result.extend(nested),
            Condition::Any(nested) if !all => result.extend(nested),
            other => result.push(other),
        }
    }
    result.sort();
    result
}

pub(super) fn name(value: &[u8], test: &NameTest, encoding: NameEncoding) -> Truth {
    let needle = match test {
        NameTest::Equal(n)
        | NameTest::Contains(n)
        | NameTest::StartsWith(n)
        | NameTest::EndsWith(n) => n,
    };
    if encoding == NameEncoding::WindowsUtf16Le
        && (!value.len().is_multiple_of(2) || !needle.len().is_multiple_of(2))
    {
        return Truth::Unknown;
    }
    let matches = match test {
        NameTest::Equal(needle) => value == needle,
        NameTest::Contains(needle) => {
            needle.is_empty()
                || value.windows(needle.len()).enumerate().any(|(i, part)| {
                    (encoding == NameEncoding::Bytes || i.is_multiple_of(2)) && part == needle
                })
        }
        NameTest::StartsWith(needle) => value.starts_with(needle),
        NameTest::EndsWith(needle) => value.ends_with(needle),
    };
    if matches {
        Truth::Match
    } else {
        Truth::NoMatch
    }
}

pub(super) fn kind(actual: ResultItemKind, expected: Kind) -> Truth {
    let actual = match actual {
        ResultItemKind::File => Kind::File,
        ResultItemKind::Directory => Kind::Directory,
        ResultItemKind::UntraversedLink => Kind::Link,
        ResultItemKind::Unknown | ResultItemKind::UnknownStructure => return Truth::Unknown,
    };
    if actual == expected {
        Truth::Match
    } else {
        Truth::NoMatch
    }
}

pub(super) fn size(value: Option<&SizeValue>, basis: Basis, boundary: Boundary) -> Truth {
    let expected = match basis {
        Basis::Logical => SizeBasis::Logical,
        Basis::Allocated => SizeBasis::Allocated,
    };
    let Some(value) = value.filter(|v| v.basis == expected && v.evidence != SizeEvidence::Unknown)
    else {
        return Truth::Unknown;
    };
    let (bytes, lower) = match &value.amount {
        SizeAmount::Exact { bytes } => (bytes, false),
        SizeAmount::LowerBound { bytes } => (bytes, true),
        SizeAmount::Unknown => return Truth::Unknown,
    };
    let Ok(n) = bytes.as_str().parse::<u64>() else {
        return Truth::Unknown;
    };
    match boundary {
        Boundary::AtLeast(x) if n >= x => Truth::Match,
        Boundary::GreaterThan(x) if n > x => Truth::Match,
        Boundary::AtMost(x) if n > x => Truth::NoMatch,
        Boundary::LessThan(x) if n >= x => Truth::NoMatch,
        Boundary::AtLeast(_) | Boundary::GreaterThan(_) if !lower => Truth::NoMatch,
        Boundary::AtMost(_) | Boundary::LessThan(_) if !lower => Truth::Match,
        _ => Truth::Unknown,
    }
}
