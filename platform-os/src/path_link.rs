//! Read-only inspection of symbolic links along an absolute path.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// The first symbolic link in a path, its target, and an optional resolved path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathLink {
    /// The symbolic-link prefix as it appeared in the inspected path.
    pub link: PathBuf,
    /// The resolved target of the link prefix, or its stored target when it cannot be resolved.
    pub target: PathBuf,
    /// The fully resolved path, or the resolved link target followed by remaining components.
    /// `None` means neither form could be resolved.
    pub suggestion: Option<PathBuf>,
}

/// Finds the first symbolic-link prefix of an absolute path without opening folders.
///
/// Only metadata is read before resolving the first link. The link's stored
/// target is used when it cannot be canonicalized. The suggestion first tries
/// to canonicalize the whole path, then the link prefix; if both fail it is
/// absent. Any metadata error before finding a link returns `None`, as does a
/// relative input path.
pub fn first_symbolic_link(path: &Path) -> Option<PathLink> {
    if !path.is_absolute() {
        return None;
    }

    let components: Vec<_> = path.components().collect();
    let mut prefix = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        prefix.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&prefix).ok()?;
        if metadata.file_type().is_symlink() {
            let target = fs::canonicalize(&prefix)
                .or_else(|_| fs::read_link(&prefix))
                .ok()?;
            let suggestion = fs::canonicalize(path).ok().or_else(|| {
                let mut suggestion = fs::canonicalize(&prefix).ok()?;
                for remainder in &components[index + 1..] {
                    suggestion.push(remainder.as_os_str());
                }
                Some(suggestion)
            });
            return Some(PathLink {
                link: prefix,
                target,
                suggestion,
            });
        }
    }
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::first_symbolic_link;
    use std::{error::Error, fs, os::unix::fs::symlink, path::Path};

    #[test]
    fn finds_a_link_in_the_middle_of_an_absolute_path() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let base = fs::canonicalize(directory.path())?;
        let target = base.join("real");
        fs::create_dir(&target)?;
        let link = base.join("link");
        symlink(&target, &link)?;
        let path = link.join("child");

        let found = first_symbolic_link(&path).ok_or("link not found")?;
        assert_eq!(found.link, link);
        assert_eq!(found.target, fs::canonicalize(target)?);
        assert_eq!(found.suggestion, Some(found.target.join("child")));
        Ok(())
    }

    #[test]
    fn finds_a_link_as_the_last_path_component() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let base = fs::canonicalize(directory.path())?;
        let target = base.join("real");
        fs::create_dir(&target)?;
        let link = base.join("link");
        symlink(&target, &link)?;

        let found = first_symbolic_link(&link).ok_or("link not found")?;
        assert_eq!(found.link, link);
        assert_eq!(found.target, fs::canonicalize(target)?);
        assert_eq!(found.suggestion.as_ref(), Some(&found.target));
        Ok(())
    }

    #[test]
    fn returns_none_when_the_path_has_no_link() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("folder");
        fs::create_dir(&path)?;

        let path = fs::canonicalize(path)?;
        assert!(first_symbolic_link(Path::new(&path)).is_none());
        Ok(())
    }

    #[test]
    fn reports_a_dangling_link_without_a_resolved_suggestion() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let base = fs::canonicalize(directory.path())?;
        let link = base.join("dangling");
        let target = base.join("missing");
        symlink(&target, &link)?;

        let found = first_symbolic_link(&link).ok_or("link not found")?;
        assert_eq!(found.link, link);
        assert_eq!(found.target, fs::read_link(&link)?);
        assert_eq!(found.suggestion, None);
        Ok(())
    }

    #[test]
    fn suggestion_resolves_a_later_link_in_the_path() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let base = fs::canonicalize(directory.path())?;
        let real = base.join("real");
        fs::create_dir_all(&real)?;
        let final_target = base.join("final");
        fs::create_dir_all(final_target.join("child"))?;
        let second_link = real.join("second");
        symlink(&final_target, &second_link)?;
        let first_link = base.join("first");
        symlink(&real, &first_link)?;
        let path = first_link.join("second").join("child");

        let found = first_symbolic_link(&path).ok_or("link not found")?;
        assert_eq!(found.link, first_link);
        assert_eq!(found.suggestion, Some(fs::canonicalize(&path)?));
        assert_eq!(found.suggestion, Some(final_target.join("child")));
        Ok(())
    }
}
