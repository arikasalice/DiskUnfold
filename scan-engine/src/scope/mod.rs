// Pure F01 scope operations over caller-verified observations.
// No OS lookups, fingerprinting, or scope persistence occur here.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Path {
    /// Only an observed, confirmed volume identity belongs here.
    pub volume: Option<Vec<u8>>,
    /// Raw entry-point identity, not a substitute for volume identity.
    pub entry: Vec<u8>,
    /// True only when this entry's correspondence to `volume` is verified.
    pub alias_verified: bool,
    /// True only when traversal to this path is known not to cross a boundary.
    pub boundary_verified: bool,
    /// Volume-relative components, already compared according to the OS rule.
    pub key: Vec<Vec<u8>>,
    /// False when the comparison rule is partial or unavailable.
    pub comparison_verified: bool,
}

fn same_domain(a: &Path, b: &Path) -> bool {
    a.volume.is_some()
        && a.volume == b.volume
        && (a.entry == b.entry || (a.alias_verified && b.alias_verified))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    ComparisonUncertain(Path),
    IdentityUncertain(Path),
    AliasUncertain(Path),
    Duplicate(Path),
    AliasMerged(Path),
    DescendantAbsorbed(Path),
    Excluded(Path),
    ExclusionUncertain(usize),
    IneffectiveExclusion(usize),
    BoundaryUncertain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    Empty,
    InvalidExclusion { index: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exclusion {
    Path(Path),
    /// Pattern and names must be supplied as already-normalized comparison text.
    NamePattern(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Normalized {
    pub includes: Vec<Path>,
    pub exclusions: Vec<Exclusion>,
    pub notices: Vec<Notice>,
}

/// Normalizes confirmed relationships only. `separators` must reflect the OS
/// path syntax; unknown separators and comparison transformations are not guessed.
pub fn normalize(
    mut includes: Vec<Path>,
    exclusions: Vec<Exclusion>,
    separators: &[char],
) -> Result<Normalized, ScopeError> {
    if includes.is_empty() {
        return Err(ScopeError::Empty);
    }
    validate_exclusions(&exclusions, separators)?;
    includes.sort_by(|a, b| {
        a.volume
            .cmp(&b.volume)
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.cmp(b))
    });
    let mut notices = Vec::new();
    for path in &includes {
        if !path.comparison_verified {
            notices.push(Notice::ComparisonUncertain(path.clone()));
        }
        if path.volume.is_none() {
            notices.push(Notice::IdentityUncertain(path.clone()));
        }
        if !path.alias_verified {
            notices.push(Notice::AliasUncertain(path.clone()));
        }
        if !path.boundary_verified {
            notices.push(Notice::BoundaryUncertain);
        }
    }
    let includes = merge(includes, &mut notices);
    let kept = apply_exclusions(includes, &exclusions, &mut notices);
    Ok(Normalized {
        includes: kept,
        exclusions,
        notices,
    })
}

fn validate_exclusions(exclusions: &[Exclusion], separators: &[char]) -> Result<(), ScopeError> {
    for (index, rule) in exclusions.iter().enumerate() {
        let invalid = match rule {
            Exclusion::Path(path) => path.entry.is_empty(),
            Exclusion::NamePattern(pattern) => {
                pattern.is_empty()
                    || pattern
                        .chars()
                        .any(|c| c == '\0' || separators.contains(&c))
            }
        };
        if invalid {
            return Err(ScopeError::InvalidExclusion { index });
        }
    }
    Ok(())
}

fn apply_exclusions(
    includes: Vec<Path>,
    exclusions: &[Exclusion],
    notices: &mut Vec<Notice>,
) -> Vec<Path> {
    let mut kept = Vec::new();
    let mut effects = vec![false; exclusions.len()];
    let mut uncertain = vec![false; exclusions.len()];
    for path in includes {
        let mut excluded = false;
        for (index, rule) in exclusions.iter().enumerate() {
            match matches_rule(rule, &path) {
                Match::Yes => {
                    excluded = true;
                    effects[index] = true;
                }
                Match::Unknown => uncertain[index] = true,
                Match::No => {}
            }
            // A child of an included path may match even when the include itself does not.
            if may_affect_descendant(rule, &path) {
                effects[index] = true;
            }
        }
        if excluded {
            notices.push(Notice::Excluded(path));
        } else {
            kept.push(path);
        }
    }
    for index in 0..exclusions.len() {
        if uncertain[index] {
            notices.push(Notice::ExclusionUncertain(index));
        } else if !effects[index] {
            notices.push(Notice::IneffectiveExclusion(index));
        }
    }
    kept
}

fn merge(paths: Vec<Path>, notices: &mut Vec<Notice>) -> Vec<Path> {
    type Key = (Vec<u8>, Vec<Vec<u8>>);
    type EntryKey = (Vec<u8>, Vec<u8>, Vec<Vec<u8>>);
    let mut same_entry: BTreeMap<EntryKey, usize> = BTreeMap::new();
    let mut aliases: BTreeMap<Key, usize> = BTreeMap::new();
    let mut kept: Vec<Path> = Vec::new();
    for path in paths {
        let mut ancestor = None;
        if let Some(volume) = &path.volume
            && path.boundary_verified
            && path.comparison_verified
        {
            for length in 0..=path.key.len() {
                let prefix = path.key[..length].to_vec();
                let entry_key = (volume.clone(), path.entry.clone(), prefix.clone());
                ancestor = same_entry.get(&entry_key).copied().or_else(|| {
                    if path.alias_verified {
                        aliases.get(&(volume.clone(), prefix)).copied()
                    } else {
                        None
                    }
                });
                if ancestor.is_some() {
                    break;
                }
            }
        }
        if let Some(index) = ancestor {
            let prior = &kept[index];
            let notice = if prior.key != path.key {
                Notice::DescendantAbsorbed(path)
            } else if prior.entry != path.entry {
                Notice::AliasMerged(path)
            } else {
                Notice::Duplicate(path)
            };
            notices.push(notice);
        } else {
            if let Some(volume) = &path.volume
                && path.boundary_verified
                && path.comparison_verified
            {
                same_entry.insert(
                    (volume.clone(), path.entry.clone(), path.key.clone()),
                    kept.len(),
                );
                if path.alias_verified {
                    aliases.insert((volume.clone(), path.key.clone()), kept.len());
                }
            }
            kept.push(path);
        }
    }
    kept
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Match {
    Yes,
    No,
    Unknown,
}

fn matches_rule(rule: &Exclusion, path: &Path) -> Match {
    match rule {
        Exclusion::Path(excluded) => {
            if excluded.volume.is_some() && path.volume.is_some() && excluded.volume != path.volume
            {
                return Match::No;
            }
            if !same_domain(excluded, path)
                || !excluded.boundary_verified
                || !path.boundary_verified
                || !excluded.comparison_verified
                || !path.comparison_verified
            {
                return Match::Unknown;
            }
            if path.key.starts_with(&excluded.key) {
                Match::Yes
            } else {
                Match::No
            }
        }
        Exclusion::NamePattern(pattern) => {
            if !path.comparison_verified {
                return Match::Unknown;
            }
            let mut uncertain = false;
            for component in &path.key {
                let Ok(name) = std::str::from_utf8(component) else {
                    uncertain = true;
                    continue;
                };
                if wildcard_matches(pattern, name) {
                    return Match::Yes;
                }
            }
            if uncertain { Match::Unknown } else { Match::No }
        }
    }
}

fn may_affect_descendant(rule: &Exclusion, path: &Path) -> bool {
    match rule {
        Exclusion::Path(excluded) => {
            same_domain(excluded, path)
                && path.boundary_verified
                && excluded.boundary_verified
                && path.comparison_verified
                && excluded.comparison_verified
                && excluded.key.starts_with(&path.key)
        }
        Exclusion::NamePattern(_) => true,
    }
}

/// '*' matches zero or more Unicode scalar values and '?' exactly one.
/// Inputs must already follow the volume's comparison and normalization rule.
pub fn wildcard_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n, mut star, mut retry) = (0, 0, None, 0);
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            retry = n;
        } else if let Some(index) = star {
            retry += 1;
            n = retry;
            p = index + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// A previously observed child mount, with identity correspondence already verified.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChildMount {
    pub path: Path,
    pub different_volume_confirmed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryMode {
    CurrentVolume,
    IncludeSelected,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Boundaries {
    pub excluded: Vec<Path>,
    pub included: Vec<Path>,
    pub uncertain: bool,
}

/// Selected points are exact observed mount paths; they never expand a volume.
pub fn boundaries(
    mut mounts: Vec<ChildMount>,
    selected: &[Path],
    mode: BoundaryMode,
    complete: bool,
) -> Boundaries {
    mounts.sort();
    mounts.dedup();
    let mut result = Boundaries {
        excluded: Vec::new(),
        included: Vec::new(),
        uncertain: !complete,
    };
    for mount in mounts {
        if !mount.different_volume_confirmed
            || mount.path.volume.is_none()
            || !mount.path.boundary_verified
        {
            result.uncertain = true;
        } else if mode == BoundaryMode::IncludeSelected && selected.contains(&mount.path) {
            result.included.push(mount.path);
        } else {
            result.excluded.push(mount.path);
        }
    }
    if mode == BoundaryMode::IncludeSelected
        && selected.iter().any(|path| !result.included.contains(path))
    {
        result.uncertain = true;
    }
    result
}

#[cfg(test)]
mod tests;
