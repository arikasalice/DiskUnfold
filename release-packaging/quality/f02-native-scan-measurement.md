# F02 native scan measurement

The opt-in Windows test `history::tests::measurement::native_scan_100k` uses the
desktop scan scheduler and real Windows directory adapter without opening a
window. It creates a temporary dataset and temporary application database and
removes both when finished. From the repository root, install the pinned web
dependencies with lifecycle scripts disabled and build the embedded assets,
then run:

```powershell
npm --prefix gui-web ci --ignore-scripts
npm --prefix gui-web run build
cargo test --locked -p du-app-host --features gui --lib history::tests::measurement::native_scan_100k -- --ignored --nocapture
```

The fixture has 100 directories, each with 1,000 one-byte files. Fixture
creation occurs before timing. The test uses the default scan configuration and
an unoptimized Rust test build. It performs two successive scans of the same
fixture; no cache flush is attempted, and the OS cache and other machine load
are uncontrolled. The second scan follows the first and is not an independent
cold-cache run. The default metadata scan config has four scan workers and the
desktop runtime's metadata I/O cap is 16. No GUI is started.

Two test invocations on 2026-11-11 used Windows 11 Pro 10.0.26200, x64, an Intel
Core i7-13700KF with 24 logical processors, and 34,103,939,072 bytes of visible
RAM. The temporary fixture was on a fixed NTFS C: volume on an NVMe WD_BLACK
SN850X 2 TB SSD. The process was not isolated from other activity. The first
version notification reported completeness `unknown`; this is the event's
published completeness classification, not a claim that no result was registered.

| Invocation / run | `scan.start` call to receipt, ms | Call to first notification dequeue, ms | Call to terminal notification dequeue, ms | First notification UTC | Terminal notification UTC | Completed entries | Failed | Unprocessed | Process lifetime peak working set, bytes |
| --- | ---: | ---: | ---: | --- | --- | ---: | ---: | ---: | ---: |
| A / 1 | 2 | 106 | 4,889 | 08:22:08.156 | 08:22:12.926 | 100,100 | 0 | 0 | 313,597,952 |
| A / 2 | 2 | 106 | 4,160 | 08:22:13.587 | 08:22:17.632 | 100,100 | 0 | 0 | 357,498,880 |
| B / 1 | 1 | 106 | 4,988 | 08:24:32.989 | 08:24:37.866 | 100,100 | 0 | 0 | 307,482,624 |
| B / 2 | 2 | 103 | 4,984 | 08:24:38.427 | 08:24:43.294 | 100,100 | 0 | 0 | 387,497,984 |

The process lifetime peak working set before scans was 34,996,224 bytes in
invocation A and 34,934,784 bytes in invocation B. Each invocation used a new
temporary fixture and a new test process.
Each peak is the Windows process high-water mark since process startup: it
includes fixture creation, the runtime, the test harness, result versions and
any previous scan. It is not a per-job allocation or the peak memory of a single
scan. The count includes the 100 directories and 100,000 files; the terminal
state was `done` and both runs also reported zero skipped entries.

These are observable measurements, not the F02 exact acceptance-to-registration
or job-start-to-terminal intervals. The start time precedes the IPC call and
the receipt follows job submission; a worker can run before the receipt.
`results.version_changed` is emitted after result registration, and
`job.finished` after terminal handling. The event transport batches for 100 ms
and this test polls at 5 ms, so dequeue times include batching, polling and
scheduling delays. UTC values are notification emission timestamps with
millisecond resolution, not the atomic registration or terminal timestamps.
No UI polling or rendering was measured.

F02 NFR1 requires scan admission and first result registration timestamps;
NFR2 requires start and end with counts; NFR3 requires scan memory high water,
channel occupancy and queued-directory counts. Exact boundary timestamps,
per-scan memory, channel occupancy and queued-directory peaks are not exposed
by this test or the current production diagnostics. The corresponding values
remain unmeasured. F02 DLD 94 Q1 leaves performance targets for a later
decision; these observations are not pass/fail judgments or release targets.
