//! Byte-exact exclusion rules for scopes whose paths use the Unix encoding.
//!
//! Path rules and names are compared as the raw bytes the platform enumerates,
//! without case folding or Unicode normalization, which is the Linux rule for
//! case-sensitive file systems. Case-insensitive volumes need a comparison rule
//! that the published design has not defined.
use super::{BoundRule, Frozen, RuleKeys, invalid, prefix::PrefixFilter};
use crate::{error::EngineError, scope::native_path};
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScopeDraft, ScopeExclusion},
    scan_observation::FilterDecision,
};
use std::sync::Arc;

/// Whether every included path, and so every rule, must use the Unix encoding.
pub(super) fn applies(draft: &ScopeDraft) -> bool {
    !draft.includes.is_empty()
        && draft
            .includes
            .iter()
            .all(|path| path.encoding == PathEncoding::Unix)
}

enum Rule {
    /// An absolute path as admitted, with its names below `/`.
    Path {
        original: OsPath,
        names: Vec<Vec<u8>>,
    },
    /// A pattern applied to single names.
    Name(Pattern),
}

impl Rule {
    fn bound(&self) -> Option<BoundRule> {
        Some(match self {
            Self::Path { original, names } => BoundRule {
                original: ScopeExclusion::Path {
                    path: original.clone(),
                },
                key: ScopeExclusion::Path { path: key(names)? },
            },
            Self::Name(pattern) => {
                let rule = ScopeExclusion::NamePattern {
                    pattern: pattern.text.clone(),
                };
                BoundRule {
                    original: rule.clone(),
                    key: rule,
                }
            }
        })
    }
}

/// Admitted Unix rules in request order.
pub(super) struct UnixRules(Vec<Rule>);

impl UnixRules {
    /// Validates every rule without filesystem access.
    ///
    /// A malformed path keeps `path.unresolvable`. A path in another encoding
    /// or with a `.` or `..` name, and an empty pattern or one containing `/`
    /// or NUL, are `scope.exclude_invalid`.
    pub(super) fn admit(rules: &[ScopeExclusion]) -> Result<Self, EngineError> {
        rules
            .iter()
            .map(admit_rule)
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

fn admit_rule(rule: &ScopeExclusion) -> Result<Rule, EngineError> {
    match rule {
        ScopeExclusion::Path { path } => {
            let checked = native_path(path)?;
            if checked.encoding != PathEncoding::Unix {
                return Err(invalid());
            }
            let raw = checked.raw.bytes().map_err(|_| invalid())?;
            let names = names(&raw).ok_or_else(invalid)?;
            Ok(Rule::Path {
                original: checked,
                names,
            })
        }
        ScopeExclusion::NamePattern { pattern } => {
            Pattern::new(pattern).map(Rule::Name).ok_or_else(invalid)
        }
    }
}

impl RuleKeys for UnixRules {
    fn freeze(&self, roots: &[(OsPath, Vec<u8>)]) -> Option<Frozen> {
        let root_names = roots
            .iter()
            .map(|(path, _)| canonical_names(path))
            .collect::<Option<Vec<_>>>()?;
        let paths: Vec<&[Vec<u8>]> = self
            .0
            .iter()
            .filter_map(|rule| match rule {
                Rule::Path { names, .. } => Some(names.as_slice()),
                Rule::Name(_) => None,
            })
            .collect();
        let patterns: Vec<Pattern> = self
            .0
            .iter()
            .filter_map(|rule| match rule {
                Rule::Name(pattern) => Some(pattern.clone()),
                Rule::Path { .. } => None,
            })
            .collect();
        let mut kept = Vec::new();
        for (index, names) in root_names.iter().enumerate() {
            if !root_excluded(&paths, &patterns, names)? {
                kept.push(index);
            }
        }
        // Patterns can match unseen descendants, so only path rules can be ineffective.
        let ineffective_rule = paths.iter().any(|rule| {
            !root_names
                .iter()
                .any(|root| root.starts_with(rule) || rule.starts_with(root))
        });
        let root_keys = root_names
            .iter()
            .map(|names| key(names))
            .collect::<Option<Vec<_>>>()?;
        let sort_keys = root_keys
            .iter()
            .map(|key| key.raw.bytes().ok())
            .collect::<Option<Vec<_>>>()?;
        kept.sort_by(|&a, &b| sort_keys[a].cmp(&sort_keys[b]));
        let per_root: Vec<_> = kept
            .iter()
            .map(|&index| {
                let root = root_names[index].as_slice();
                PrefixFilter::new(paths.iter().filter_map(|rule| rule.strip_prefix(root)))
            })
            .collect();
        let rules = self.0.iter().map(Rule::bound).collect::<Option<Vec<_>>>()?;
        let filter = Arc::new(move |index: u32, parent: &[Vec<u8>], name: &[u8]| {
            let Some(paths) = per_root.get(index as usize) else {
                return FilterDecision::Undecidable;
            };
            match paths.decide(parent, name, |_, raw| Some(raw.to_vec())) {
                FilterDecision::Admit => decide_name(&patterns, name),
                decision => decision,
            }
        });
        Some(Frozen {
            root_keys: kept.iter().map(|&index| root_keys[index].clone()).collect(),
            roots: kept,
            rules,
            ineffective_rule,
            filter,
        })
    }

    fn root_key(&self, canonical: &OsPath, _: &[u8]) -> Option<OsPath> {
        key(&canonical_names(canonical)?)
    }
}

/// A root is excluded when a path rule covers it or a pattern matches one of its
/// names; `None` when a pattern cannot decide one of those names.
fn root_excluded(paths: &[&[Vec<u8>]], patterns: &[Pattern], names: &[Vec<u8>]) -> Option<bool> {
    if paths.iter().any(|rule| names.starts_with(rule)) {
        return Some(true);
    }
    let mut undecidable = false;
    for name in names {
        match decide_name(patterns, name) {
            FilterDecision::Exclude => return Some(true),
            FilterDecision::Undecidable => undecidable = true,
            FilterDecision::Admit => {}
        }
    }
    (!undecidable).then_some(false)
}

/// Excludes a name that any pattern matches, even if another pattern cannot decide.
fn decide_name(patterns: &[Pattern], name: &[u8]) -> FilterDecision {
    let mut undecidable = false;
    for pattern in patterns {
        match pattern.matches(name) {
            Some(true) => return FilterDecision::Exclude,
            Some(false) => {}
            None => undecidable = true,
        }
    }
    if undecidable {
        FilterDecision::Undecidable
    } else {
        FilterDecision::Admit
    }
}

fn canonical_names(path: &OsPath) -> Option<Vec<Vec<u8>>> {
    if path.encoding != PathEncoding::Unix {
        return None;
    }
    names(&path.raw.bytes().ok()?)
}

/// Splits an absolute path into its names, ignoring empty ones.
///
/// `.` and `..` are rejected so a rule never depends on resolving links.
fn names(raw: &[u8]) -> Option<Vec<Vec<u8>>> {
    raw.strip_prefix(b"/")?
        .split(|&byte| byte == b'/')
        .filter(|name| !name.is_empty())
        .map(|name| (name != b"." && name != b"..").then(|| name.to_vec()))
        .collect()
}

/// Serializes names as the absolute path used for hashing and ordering.
fn key(names: &[Vec<u8>]) -> Option<OsPath> {
    let mut raw = Vec::new();
    for name in names {
        raw.push(b'/');
        raw.extend_from_slice(name);
    }
    if raw.is_empty() {
        raw.push(b'/');
    }
    native_path(&OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(&raw),
        display: String::new(),
        lossy: false,
    })
    .ok()
}

enum Token<T> {
    /// `*`: any run of units, possibly empty.
    Any,
    /// `?`: exactly one unit.
    One,
    Unit(T),
}

#[derive(Clone)]
enum Tokens {
    /// Without `?`, bytes match exactly as characters would in UTF-8 names.
    Bytes(Arc<[Token<u8>]>),
    /// With `?`, a unit is one Unicode scalar value.
    Chars(Arc<[Token<char>]>),
}

/// A name pattern: `*` matches any run within one name and `?` one character.
#[derive(Clone)]
pub(super) struct Pattern {
    text: String,
    tokens: Tokens,
}

impl Pattern {
    /// Accepts a nonempty pattern without `/` or NUL; there is no escape syntax.
    pub(super) fn new(text: &str) -> Option<Self> {
        if text.is_empty() || text.contains(['/', '\0']) {
            return None;
        }
        let tokens = if text.contains('?') {
            Tokens::Chars(
                text.chars()
                    .map(|unit| match unit {
                        '*' => Token::Any,
                        '?' => Token::One,
                        unit => Token::Unit(unit),
                    })
                    .collect(),
            )
        } else {
            Tokens::Bytes(
                text.bytes()
                    .map(|unit| match unit {
                        b'*' => Token::Any,
                        unit => Token::Unit(unit),
                    })
                    .collect(),
            )
        };
        Some(Self {
            text: text.to_owned(),
            tokens,
        })
    }

    /// Matches one raw name, or `None` when `?` cannot count a non-UTF-8 name.
    pub(super) fn matches(&self, name: &[u8]) -> Option<bool> {
        match &self.tokens {
            Tokens::Bytes(tokens) => Some(wildcard(tokens, name)),
            Tokens::Chars(tokens) => {
                let name: Vec<char> = std::str::from_utf8(name).ok()?.chars().collect();
                Some(wildcard(tokens, &name))
            }
        }
    }
}

/// Matches with backtracking to the most recent `*`; a literal `*` in a name
/// is matched by a pattern `*` like any other unit.
fn wildcard<T: PartialEq>(pattern: &[Token<T>], name: &[T]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut resume = None;
    while n < name.len() {
        match pattern.get(p) {
            Some(Token::Any) => {
                resume = Some((p + 1, n));
                p += 1;
            }
            Some(Token::One) => {
                p += 1;
                n += 1;
            }
            Some(Token::Unit(unit)) if *unit == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match resume {
                Some((after, start)) => {
                    resume = Some((after, start + 1));
                    p = after;
                    n = start + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|token| matches!(token, Token::Any))
}
