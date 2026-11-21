# F03 result index memory measurement

This note records the reproducible measurements for the F03 result index and the real-filesystem tree shared with F02. The generated files are empty: this measures directory enumeration and result-index memory without adding file-content I/O.

## Create and remove a filesystem tree

Build the standard-library-only generator:

```sh
cargo build --locked --release -p du-release-tools --bin make-measure-tree
```

Create a new tree under a parent directory that already exists:

```sh
target/release/make-measure-tree create /private/tmp/diskunfold-f03-328-tree \
  --width 10 --depth 5 --files-per-directory 17 --name-length 12
```

`width` is the number of child directories per directory above the requested depth. The root is depth zero. Every directory, including the root, receives `files-per-directory` empty files. Directory and file names use distinct `d` and `f` prefixes and have exactly `name-length` ASCII bytes. The command reports directory, file and total item counts before a measurement run. It refuses an existing tree or marker. A marker is stored beside the tree so the scanner does not count it.

Remove only a tree created by this tool:

```sh
target/release/make-measure-tree remove /private/tmp/diskunfold-f03-328-tree
```

Removal checks that the target is a directory and that its sidecar marker names the same canonical path. The tree root and its parent must not be replaced while a run is in progress.

The example dimensions produce 111,111 directories and 1,888,887 files, or 1,999,998 scanned items. Adjust the four parameters to exercise other widths, depths, directory file counts and name lengths. F02 can use the same generator when it needs a real filesystem tree; its synthetic harness remains useful for cases that cannot be built cheaply on disk.

## Run the release CLI

Build the CLI-only release binary, then scan the canonical path with an isolated data directory:

```sh
cargo build --locked --release -p du-launcher --no-default-features
/usr/bin/time -l env DISKUNFOLD_DATA_DIR=/private/tmp/diskunfold-f03-328-data \
  target/release/diskunfold --lang en scan --format json \
  /private/tmp/diskunfold-f03-328-tree
```

Record wall time and `maximum resident set size` from `/usr/bin/time -l`, together with the generator item count and the CLI's completed/failed counts. The data directory must be separate from the measured tree. The CLI command does not open the GUI or request elevated privileges. Remove the generated tree and isolated data directory after collecting the result.

## In-memory index harness

The ignored `du-analysis-index` test builds a deterministic synthetic mixed tree, publishes it, then seals it while retaining the immutable result. Run exactly one process per case:

```sh
cargo test --locked --release -p du-analysis-index --test index_measure --no-run
F03_MEASURE_TREE=mixed F03_MEASURE_DIRS=80000 \
  target/release/deps/index_measure-<hash> --ignored --nocapture
F03_MEASURE_TREE=mixed F03_MEASURE_DIRS=80000 F03_MEASURE_EVIDENCE=1 \
  target/release/deps/index_measure-<hash> --ignored --nocapture
cargo test --locked --release -p du-analysis-index --lib memory_layout_measure -- \
  --ignored --nocapture
```

`F03_MEASURE_DIRS=80000` currently produces 1,000,381 items, including directories and files. The first run measures the sealed result without space evidence; the second adds allocation, identity, link count and observation time for every file. `record_resident`, `publish_resident` and `seal_resident` are current RSS samples from `ps`; divide by `items` for bytes per item. The seal sample includes allocator-retained pages and should be compared on the same host and toolchain. The layout test prints fixed-size Rust types separately. These figures do not include the release CLI, filesystem metadata cache, or the OS page cache.

After a change, run the full affected package tests on macOS:

```sh
cargo test --locked -p du-analysis-index -p du-engine-api -p du-scan-engine
```

Keep the generated tree, isolated data and build target outside the source checkout. Report synthetic harness memory and real CLI peak RSS separately; they cover different phases and inputs.
