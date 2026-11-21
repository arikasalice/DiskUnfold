use std::{
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

const MARKER_SUFFIX: &str = ".diskunfold-measure-tree";
const MARKER_CONTENT: &str = "diskunfold-measure-tree-v1\n";

struct TreeSpec {
    width: u32,
    depth: u32,
    files_per_directory: u32,
    name_length: usize,
}

enum Command {
    Create(PathBuf, TreeSpec),
    Remove(PathBuf),
}

fn main() -> ExitCode {
    match parse(env::args_os().skip(1)).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("make-measure-tree: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mode = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    let root = args.next().map(PathBuf::from).ok_or_else(usage)?;
    if mode == "remove" {
        if args.next().is_some() {
            return Err(usage());
        }
        return Ok(Command::Remove(root));
    }
    if mode != "create" {
        return Err(usage());
    }

    let mut width = None;
    let mut depth = None;
    let mut files_per_directory = None;
    let mut name_length = None;
    while let Some(option) = args.next() {
        let option = option.into_string().map_err(|_| usage())?;
        let value = args
            .next()
            .and_then(|value| value.into_string().ok())
            .ok_or_else(usage)?;
        match option.as_str() {
            "--width" => set_once(&mut width, parse_number(&value, &option)?)?,
            "--depth" => set_once(&mut depth, parse_number(&value, &option)?)?,
            "--files-per-directory" => {
                set_once(&mut files_per_directory, parse_number(&value, &option)?)?
            }
            "--name-length" => set_once(&mut name_length, parse_number(&value, &option)?)?,
            _ => return Err(usage()),
        }
    }
    let spec = TreeSpec {
        width: width.ok_or_else(usage)?,
        depth: depth.ok_or_else(usage)?,
        files_per_directory: files_per_directory.ok_or_else(usage)?,
        name_length: name_length.ok_or_else(usage)?,
    };
    spec.validate()?;
    Ok(Command::Create(root, spec))
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err(usage());
    }
    Ok(())
}

fn parse_number<T: std::str::FromStr>(value: &str, option: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for {option}: {value}"))
}

fn usage() -> String {
    "usage: make-measure-tree create <path> --width N --depth N --files-per-directory N --name-length N | remove <path>".into()
}

impl TreeSpec {
    fn validate(&self) -> Result<(u64, u64), String> {
        if !(2..=255).contains(&self.name_length) {
            return Err("name length must be between 2 and 255 bytes".into());
        }
        let index_capacity = self.name_length - 1;
        for (label, maximum) in [
            ("width", self.width.saturating_sub(1)),
            (
                "files per directory",
                self.files_per_directory.saturating_sub(1),
            ),
        ] {
            if maximum.to_string().len() > index_capacity {
                return Err(format!("{label} does not fit the requested name length"));
            }
        }
        let mut directories = 1_u64;
        let mut level = 1_u64;
        for _ in 0..self.depth {
            level = level
                .checked_mul(u64::from(self.width))
                .ok_or_else(|| "tree item count overflows u64".to_owned())?;
            directories = directories
                .checked_add(level)
                .ok_or_else(|| "tree item count overflows u64".to_owned())?;
        }
        let files = directories
            .checked_mul(u64::from(self.files_per_directory))
            .ok_or_else(|| "tree item count overflows u64".to_owned())?;
        directories
            .checked_add(files)
            .ok_or_else(|| "tree item count overflows u64".to_owned())?;
        Ok((directories, files))
    }
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Create(root, spec) => create(&root, &spec),
        Command::Remove(root) => remove(&root),
    }
}

fn create(root: &Path, spec: &TreeSpec) -> Result<(), String> {
    let (directories, files) = spec.validate()?;
    let parent = root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| error.to_string())?;
    let name = root
        .file_name()
        .ok_or_else(|| "tree path must name a new directory".to_owned())?;
    let root = parent.join(name);
    let marker = marker_path(&root)?;
    if fs::symlink_metadata(&root).is_ok() || fs::symlink_metadata(&marker).is_ok() {
        return Err("tree path or its marker already exists".into());
    }
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    if let Err(error) = write_marker(&marker, &root) {
        let _ = fs::remove_dir(&root);
        return Err(error);
    }

    let mut pending = vec![(root.clone(), 0_u32)];
    while let Some((directory, level)) = pending.pop() {
        for index in 0..spec.files_per_directory {
            File::create(directory.join(name_for('f', index, spec.name_length)))
                .map_err(|error| error.to_string())?;
        }
        if level < spec.depth {
            for index in 0..spec.width {
                let child = directory.join(name_for('d', index, spec.name_length));
                fs::create_dir(&child).map_err(|error| error.to_string())?;
                pending.push((child, level + 1));
            }
        }
    }
    println!(
        "F03_MEASURE_TREE path={} directories={directories} files={files} items={}",
        root.display(),
        directories + files
    );
    Ok(())
}

fn name_for(prefix: char, index: u32, length: usize) -> String {
    format!("{prefix}{index:0width$}", width = length - 1)
}

fn marker_path(root: &Path) -> Result<PathBuf, String> {
    let parent = root
        .parent()
        .ok_or_else(|| "tree path must have a parent directory".to_owned())?;
    let name = root
        .file_name()
        .ok_or_else(|| "tree path must name a directory".to_owned())?
        .to_string_lossy();
    Ok(parent.join(format!(".{name}{MARKER_SUFFIX}")))
}

fn write_marker(marker: &Path, root: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)
        .map_err(|error| error.to_string())?;
    write!(file, "{MARKER_CONTENT}{}\n", root.display()).map_err(|error| error.to_string())
}

fn remove(root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_dir() {
        return Err("refusing to remove a non-directory tree path".into());
    }
    let parent = fs::canonicalize(root.parent().unwrap_or_else(|| Path::new(".")))
        .map_err(|error| error.to_string())?;
    let canonical_root = parent.join(
        root.file_name()
            .ok_or_else(|| "tree path must name a directory".to_owned())?,
    );
    if canonical_root == Path::new("/") {
        return Err("refusing to remove a filesystem root".into());
    }
    let marker = marker_path(&canonical_root)?;
    let expected = format!("{MARKER_CONTENT}{}\n", canonical_root.display());
    if fs::read_to_string(&marker).map_err(|error| error.to_string())? != expected {
        return Err("tree marker is missing or does not match this path".into());
    }
    fs::remove_dir_all(&canonical_root).map_err(|error| error.to_string())?;
    fs::remove_file(marker).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::TreeSpec;

    #[test]
    fn counts_a_fixed_width_tree() {
        let spec = TreeSpec {
            width: 10,
            depth: 5,
            files_per_directory: 17,
            name_length: 8,
        };
        assert_eq!(
            spec.validate().expect("valid dimensions"),
            (111_111, 1_888_887)
        );
    }
}
