# F01 Q8: Rust service and SQLite measurements

This is a candidate-selection experiment, not an approved retention value or a
release performance gate. F01 DLD 102 §2.3.2, §2.6.1 and §2.6.2 require
measuring summary lists, body sizes, duplicate updates and over-cap eviction;
Q8 delegates the value to the architecture DLD after measurement. HLD 111
requires the owner to persist only a successfully admitted GUI scope. This
experiment does not implement or exercise that admission path.

Run on Windows, from the repository root, after building the GUI assets as
required by `app-host` (the benchmark does not start the GUI):

```powershell
npm --prefix gui-web ci --ignore-scripts
npm --prefix gui-web run build
cargo run --release --locked -p du-app-host --features gui --example q8_recent
```

The example creates a new temporary directory and database per scenario and
removes it afterward. Its fixture-only repository adapter calls the actual
`RecentService` and `RecentScopeStore`; there is no production app-host
composition yet. A precheck, outside the timed region, verifies that updating
the same fingerprint preserves the first ID and body and that insertion past
the fixture limit removes the oldest row.

Synthetic logical inputs are one Windows folder (380 JSON bytes), a long Korean
path (1,275 bytes), and 12 roots with 40 path exclusions (11,213 bytes). The
fixture contains `OsPath` raw UTF-16LE and display values but its enclosing JSON
field names, volume identifier and validator are **not** a published physical
format. Paths are not created or resolved; this measures persistence, not OS
scope validation. Repeated rows share the same body and differ in deterministic
sequential UUID-v7-shaped ID, fingerprint and time. The limit values are
experimental, not defaults. Each scenario seeds exactly its cap, then executes
20 warmups and 200 measured successful operations per column. Percentile is
nearest-rank p95, elapsed wall clock. Busy errors are counted, not included in
latency; other errors terminate the experiment.

Write timings include constructing the candidate and copying the synthetic
body inside the timed closure. The injected fixture validator parses JSON in the
service and again in the SQLite adapter. Summary-list timings do not load bodies.

The SQLite adapter opens WAL mode with synchronous NORMAL and a 5,000 ms busy
timeout; automatic checkpointing remains enabled. The concurrent columns use
two independent database connections, one list thread and one insert-and-evict
thread, synchronized at the start of each pair of operations. The barrier does
not prove the SQL calls overlap in time. The benchmark has no competing writer,
settings changes, checkpoint control, GUI, or interprocess load. Page bytes are
`page_count * page_size` as reported by SQLite. File sizes are observed after the
concurrent reader exits while the writer and inspection connection remain open;
WAL size is a point sample affected by automatic checkpoints, not a peak. They
include database/schema overhead and exclude the shared-memory file and other
application files.

One run on Windows 11 Pro, local C: NTFS, Intel Core i7-13700KF, Rust 1.99.1
release build, bundled SQLite 3.54.1. Every column had 200 samples and no BUSY
results (including warmups). Values are milliseconds unless labelled bytes.

| Fixture | Cap | List p95 | Duplicate p95 | Insert+evict p95 | Paired list p95 | Paired insert+evict p95 | Page bytes | DB file bytes | WAL bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| single | 50 | 0.068 | 0.160 | 0.240 | 0.116 | 0.227 | 172,032 | 172,032 | 4,144,752 |
| single | 100 | 0.061 | 0.091 | 0.176 | 0.135 | 0.213 | 204,800 | 204,800 | 4,136,512 |
| single | 500 | 0.215 | 0.186 | 0.275 | 0.358 | 0.349 | 425,984 | 425,984 | 4,227,152 |
| korean_long | 50 | 0.038 | 0.101 | 0.184 | 0.107 | 0.187 | 217,088 | 217,088 | 4,148,872 |
| korean_long | 100 | 0.052 | 0.089 | 0.180 | 0.166 | 0.207 | 294,912 | 294,912 | 4,152,992 |
| korean_long | 500 | 0.201 | 0.173 | 0.298 | 0.814 | 0.751 | 880,640 | 880,640 | 6,237,712 |
| multi | 50 | 0.039 | 0.179 | 0.469 | 0.760 | 0.776 | 770,048 | 770,048 | 4,350,752 |
| multi | 100 | 0.117 | 0.318 | 0.312 | 1.040 | 0.900 | 1,392,640 | 1,392,640 | 4,474,352 |
| multi | 500 | 5.746 | 2.799 | 1.764 | 5.627 | 3.835 | 6,348,800 | 6,348,800 | 8,911,592 |

A later run concurrent with a Clippy build sampled a 16,133,952-byte WAL for
the 500-row multi case; it is not the table run and demonstrates why this
single post-workload file length is not a stable peak or a storage bound.

The earlier Python SQL reproduction in `recent-scope-measurement.md` selected
full rows and parsed each body. It is not comparable to this Rust summary-only
list, which does not read bodies. One warm run on one fast machine cannot set a
production p95 bound, forecast slow or remote storage, or predict actual body
size distribution. Multiple runs showed variable results under shared-machine
load. The experiment also cannot establish end-to-end IPC latency, cancellation,
setting/write ordering, crashes, or a hard disk-space bound.

## Proposed architecture DLD wording for review, not a decision

> The recent-scope storage owner receives a positive retention count through
> composition and applies it to list, selected-item read, and atomic upsert plus
> eviction. For 0.1, evaluate 100 rows as the provisional default; permit a
> deployment-provided count after validation. Do not infer an unlimited body
> budget from the row count. Confirm the default and the allowable configurable
> range only after measuring representative admitted bodies, slow supported
> devices, end-to-end list latency, WAL/checkpoint behavior, concurrent settings
> and writes, and three-platform failure paths.

The measured 100-row multi fixture used 1,392,640 page bytes and a 4,474,352-byte
WAL at the sampling point; the 500-row fixture used 6,348,800 page bytes and an
8,911,592-byte WAL. This suggests 100 as a review candidate, not a proven safe
maximum: neither per-scope body size nor a latency acceptance threshold is
published. The configurable upper range therefore remains an explicit decision.

For immediate remember-setting semantics, the production owner needs one
serialization boundary covering setting changes, current setting observation,
cancel/admission state and the SQLite write/commit. A single owner queue could
serialize these requests: acknowledge an off change only after earlier writes
finish, and check the current value and cancellation within the same ordered
write operation. The owner must define the commit-versus-cancel linearization
point and never report cancellation as undoing an already committed write. This
is a proposed integration plan; the existing snapshot-based test service has no
such synchronization and this benchmark does not prove it.
