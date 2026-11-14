# F02 lane queue capacity measurement

This note records how the parallel metadata collector behaves with different lane queue capacities, worker counts and storage lanes, and what bounds the memory the collector holds. It supplies measured evidence for `QUEUED_DIRECTORIES` (NFR3.HD1.app-host.DD1, provisional value 64, no value in the published design), for the per-storage lanes (NFR2.HD1.app-host.DD1) and for collector throughput (NFR1). No performance target is asserted.

## Findings

- **Capacity does not change throughput between 8 and 1,024.** On wide, deep and mixed synthetic trees, with 1 to 16 workers, 1 or 4 lanes and read delays up to 1 ms, every capacity from 8 to 1,024 gave the same collector throughput within run-to-run noise (about ±10 % on Windows). With an accurate 1 ms read delay on Linux, throughput scaled from 5,400 items/s with one worker to 84,000 items/s with 16 workers at every capacity, including 8. The result did not depend on the capacity relative to the worker count, so a worker-proportional rule has no support.
- **An unbounded queue is worse.** On Windows it was up to 35 % slower (typically 10–25 %) on wide, mixed and full-batch trees and equal on deep chains. On the one-million-directory tree it queued up to 113,932 directories with 80,143 synthetic handles open at once (a real adapter would hold that many OS directory handles), and the process peak grew from about 100 MiB to 527 MiB.
- **The worst-case collector memory grows with lanes × capacity × read batch, and runs reach it.** With 8,192-entry batches (the largest batch the collector accepts), 16 lanes, 16 workers and capacity 64, the process peaked at 1,918 MiB while 7.4 million returned entries were held, 89 % of 16 × 64 × 8,192. Capacity 8 peaked at 325 MiB. With the 256-entry batches the platform adapters return, the same configuration peaked at 86 MiB.
- **The result index, not the lane queue, dominates memory and time at the production entry point.** Through `collect_open_forest_diagnostic` with the result index and timed partial publication, a 1,258,623-item tree took 25–33 s and peaked at about 2.8 GiB at every capacity; the collector alone took 0.7–3.6 s and peaked at 33–87 MiB. Extrapolated, a 12.5-million-item scan would need tens of GiB. Full-entry time also grows quickly with depth: 0.8 s, 6.3 s and 103 s for single chains of depth 1,000, 2,000 and 4,000 with 5 to 20 thousand items, against 0.6 s for depth 4,000 in the collector alone (single runs with one worker).
- **Throughput falls above four workers.** All workers share one scheduler mutex taken several times per returned entry. On Windows the mixed tree peaked at about 1.8 million items/s with four workers and fell to 0.9–1.0 million with 16; one wide directory fell from 1.3 million items/s with two to four workers to 0.25 million with 16. In the Linux container (Docker on WSL2) the same runs were 2–40 times slower and highly variable, with kernel time above user time in a sampled interval, which is consistent with lock convoys on the futex path. Native Linux was not measured.

The capacity stays at 64. No measured value is faster, and a smaller value would lower only the worst-case bound that 8,192-entry batches create. That bound is better reduced directly; see [Possible reductions](#possible-reductions). The capacity also sets how much work is taken shallowest-first from the lane queue before it continues depth-first from worker stacks. The order in which results appear was not measured, so changing it needs evidence that this note does not provide.

## Running the measurement

The harness is a pair of ignored tests in [measure.rs](../../scan-engine/src/scan/parallel/tests/lanes/measure.rs). `queue_capacity_grid` starts one process of the same test binary for every run, so each process peak belongs to one collection. `queue_capacity_run` performs that collection. Build with optimization and run the grid:

```sh
cargo test --locked --release -p du-scan-engine --lib --no-run
F02_MEASURE_PLAN=main F02_MEASURE_REPS=3 \
  <target>/release/deps/du_scan_engine-<hash> --exact \
  scan::parallel::tests::lanes::measure::queue_capacity_grid --ignored --nocapture
```

In PowerShell set `$env:F02_MEASURE_PLAN` and `$env:F02_MEASURE_REPS` before running the same binary. `cargo test --release ... -- --ignored --exact <name> --nocapture` also works. `F02_MEASURE_FILTER` keeps only configurations whose description contains every space-separated token, for example `"delay_us=1000 workers=4 limit"`.

| Plan | Contents |
| --- | --- |
| `smoke` | A few small configurations to check the harness, in seconds |
| `main` | Wide, deep and mixed trees; 1 and 4 lanes; workers 1, 2, 4, 8, 16 (collector) or 1, 4, 16 (full); capacities 8, 16, 64, 256, 1,024 and unbounded |
| `delay` | Mixed 3,000-directory tree with a fixed sleep of 0, 50 or 1,000 µs before every directory read; workers 1, 4, 16; every capacity |
| `batch` | Every directory holds 8 subdirectories and one full read batch of files; batches of 256 and 8,192 entries; 1, 4 and 16 lanes; workers 4 and 16; capacities 8, 64 and unbounded |
| `large` | Idle baseline; mixed tree of one million directories (collector) and 100,000 directories (full) |

Each run prints `F02_MEASURE_RUN` with its configuration and measurements, and each configuration prints `F02_MEASURE_MEDIAN`. `F02_MEASURE_HOST` and `F02_MEASURE_END` record the host load when the plan starts and ends. Runs that fail print `F02_MEASURE_FAIL`, and the grid then fails.

## Method

**Trees.** A synthetic tree is computed from directory numbers instead of being stored ([tree.rs](../../scan-engine/src/scan/parallel/tests/lanes/measure/tree.rs)). Directories are numbered in preorder, and each knows its number of descendant directories, so only open handles hold their child lists. Every item has a distinct nonzero identity on one device, files are one byte, names are short ASCII (`d12`, `f3`), subdirectories are listed before files and a read returns at most `batch` entries.

| Tree | Directories | Items collected | Deepest directory |
| --- | ---: | ---: | ---: |
| wide: one directory with 50,000 subdirectories, 4 files in every directory | 50,001 | 250,004 | 1 |
| deep: one chain, 4 files per directory | 4,001 | 20,004 | 4,000 |
| mixed: 70 % of directories with 1–4 subdirectories, 25 % with 5–20 and 5 % with 21–2,000 (fewer in small subtrees); 11.5 files per directory on average with a long tail | 100,001 | 1,258,623 | 36 |
| mixed, large | 1,000,001 | 12,546,944 | 45 |
| full batch: 8 subdirectories and `batch` files per directory | 2,001 (256) or 201 (8,192) per lane | 514,256 or 1,646,792 per lane | 4 or 3 |

With four lanes the main plan divides the directories of the wide, deep and mixed trees over four roots, each in its own device group, so the deep chains are 1,000 deep. The full runs of the main plan use 20,000 mixed directories and a 1,000-directory chain, because index time grows quickly with size and depth. The batch plan gives each lane its own tree, so 16 lanes hold 16 times the items.

**Modes.** `collector` runs the production worker, scheduler and lane code (`spawn_workers`, `start_queue`, the lane queues and stealing) and a consumer that counts entries and drops them; no result index exists. `full` runs `collect_open_forest_diagnostic`, the entry point `MetadataScan` uses, into an analysis-index build. Its job context supplies a monotonic clock, so partial results are published on the production cadence. The per-lane worker limit equals the worker count; production uses four workers and a limit of four.

**Measurements.** Throughput is collected items divided by the wall time of the collection call; a full run includes sealing the result. The queue peak is `ScanDiagnostics::shared_ready_peak` (full) or the same lane counter (collector): the most items held in all lane queues at once, initial roots included. The deepest stack is the most items on one worker stack and all stacks the most items on all stacks at once. The unrecorded entries peak is the most entries returned by reads and not yet recorded by the consumer: the read batches that frames hold, the 8,192-event channel and entries in flight. The open handles peak counts synthetic directory handles. The process peak is the high-water mark since process start: `PeakWorkingSet64` read through PowerShell on Windows and `VmHWM` from `/proc/self/status` on Linux, without unsafe code. An idle run peaked at 5–31 MiB on Windows. Every configuration ran three times; each column is the median of that measurement, so values in one row can come from different runs.

The collector and the result index are separated only by mode: the collector runs have no index, so the difference between the modes approximates the index, the publisher and its partial publications. The memory inside the collector (lane queues, stacks, channel, handles) is not separated by measurement; the unrecorded entries and handle peaks show which part grew.

**Build.** The tests run in the release profile with the test configuration. Test builds count index comparisons in thread-local cells and add one relaxed atomic update per worker stack change. They also read a capacity override in `scheduler::lane_capacity`. Release builds contain none of these.

**Hosts.** Windows 11 Pro 10.0.26200 on an Intel Core i7-13700KF (24 logical processors, 34,103,939,072 bytes of RAM), rustc 1.99.1, measured from 2026-11-15 00:38 to 01:41 KST. The machine was shared. The processor load was 23 % at the start of the main plan and 21 % at its end. Another test process from a different build was running at least during the 1 ms delay plan; the load at the starts and ends of the 1 ms delay, batch and large plans was between 28 % and 100 %. Linux: Ubuntu 24.04.5 container `f15-linux-check-496c-u2404` on the same host under Docker with the WSL2 kernel 6.18.33.2, 24 visible CPUs without a CPU quota, 15.5 GiB of memory and rustc 1.99.1; the one-minute load average was 2.5–3.1 at the start of a plan and 0.2 at the end of the delay plan.

## Windows results

### Collector throughput

Thousand items per second, no read delay, 256-entry batches.

| Tree | Lanes | Workers | 8 | 16 | 64 | 256 | 1024 | unbounded |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| wide | 1 | 1 | 808 | 900 | 906 | 898 | 906 | 817 |
| wide | 1 | 2 | 1,306 | 1,322 | 1,355 | 1,344 | 1,368 | 1,160 |
| wide | 1 | 4 | 1,273 | 1,180 | 1,328 | 1,279 | 1,200 | 1,035 |
| wide | 1 | 8 | 692 | 722 | 768 | 803 | 769 | 746 |
| wide | 1 | 16 | 243 | 238 | 251 | 255 | 255 | 247 |
| wide | 4 | 1 | 760 | 845 | 861 | 862 | 886 | 781 |
| wide | 4 | 2 | 1,223 | 1,356 | 1,339 | 1,382 | 1,313 | 1,225 |
| wide | 4 | 4 | 1,399 | 1,391 | 1,353 | 1,384 | 1,254 | 1,299 |
| wide | 4 | 8 | 899 | 897 | 857 | 801 | 846 | 761 |
| wide | 4 | 16 | 718 | 663 | 688 | 652 | 592 | 608 |
| deep | 1 | 1 | 6 | 6 | 6 | 5 | 8 | 9 |
| deep | 1 | 2 | 18 | 18 | 19 | 19 | 19 | 18 |
| deep | 1 | 4 | 36 | 35 | 36 | 37 | 37 | 36 |
| deep | 1 | 8 | 27 | 27 | 27 | 35 | 24 | 37 |
| deep | 1 | 16 | 35 | 37 | 38 | 29 | 32 | 34 |
| deep | 4 | 1 | 28 | 27 | 28 | 28 | 28 | 28 |
| deep | 4 | 2 | 60 | 61 | 61 | 61 | 60 | 59 |
| deep | 4 | 4 | 133 | 144 | 144 | 146 | 152 | 148 |
| deep | 4 | 8 | 94 | 96 | 90 | 88 | 89 | 88 |
| deep | 4 | 16 | 65 | 67 | 61 | 63 | 51 | 57 |
| mixed | 1 | 1 | 351 | 350 | 369 | 365 | 377 | 365 |
| mixed | 1 | 2 | 774 | 675 | 742 | 714 | 751 | 667 |
| mixed | 1 | 4 | 1,775 | 1,749 | 1,813 | 1,812 | 1,737 | 1,387 |
| mixed | 1 | 8 | 1,233 | 1,345 | 1,339 | 1,399 | 1,435 | 1,129 |
| mixed | 1 | 16 | 957 | 989 | 902 | 925 | 971 | 845 |
| mixed | 4 | 1 | 427 | 437 | 419 | 471 | 441 | 406 |
| mixed | 4 | 2 | 881 | 896 | 877 | 877 | 876 | 779 |
| mixed | 4 | 4 | 1,700 | 1,767 | 1,749 | 1,804 | 1,649 | 1,402 |
| mixed | 4 | 8 | 1,212 | 1,228 | 1,228 | 1,477 | 1,263 | 1,001 |
| mixed | 4 | 16 | 1,048 | 957 | 1,034 | 1,052 | 1,052 | 912 |

The deep chains keep at most ten directories queued, so the capacity never applies; their throughput is limited by copying the path, which every entry repeats at full depth.

### Collector memory, queue and stacks

| Tree | Lanes | Workers | Capacity | Peak MiB | Queue peak | Deepest stack | All stacks | Unrecorded entries peak | Open handles peak |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| wide | 1 | 4 | 8 | 9 | 8 | 1 | 4 | 299 | 12 |
| wide | 1 | 4 | 64 | 10 | 64 | 1 | 4 | 439 | 68 |
| wide | 1 | 4 | 1024 | 11 | 1,024 | 1 | 4 | 2,636 | 1,026 |
| wide | 1 | 4 | unbounded | 67 | 41,952 | 0 | 0 | 83,191 | 39,804 |
| wide | 1 | 16 | 8 | 35 | 8 | 1 | 10 | 345 | 23 |
| wide | 1 | 16 | 64 | 34 | 54 | 0 | 0 | 385 | 58 |
| wide | 1 | 16 | 1024 | 35 | 52 | 0 | 0 | 343 | 58 |
| wide | 1 | 16 | unbounded | 35 | 79 | 0 | 0 | 473 | 81 |
| wide | 4 | 4 | 8 | 14 | 32 | 1 | 4 | 1,024 | 20 |
| wide | 4 | 4 | 64 | 21 | 256 | 1 | 4 | 1,024 | 130 |
| wide | 4 | 4 | 1024 | 24 | 4,096 | 1 | 4 | 12,246 | 4,093 |
| wide | 4 | 4 | unbounded | 81 | 50,000 | 0 | 0 | 196,336 | 50,001 |
| wide | 4 | 16 | 8 | 10 | 32 | 1 | 12 | 1,836 | 48 |
| wide | 4 | 16 | 64 | 11 | 256 | 1 | 13 | 1,804 | 270 |
| wide | 4 | 16 | 1024 | 32 | 4,096 | 1 | 13 | 6,649 | 3,033 |
| wide | 4 | 16 | unbounded | 66 | 42,128 | 0 | 0 | 70,137 | 37,078 |
| mixed | 1 | 4 | 8 | 39 | 8 | 34 | 80 | 3,318 | 91 |
| mixed | 1 | 4 | 64 | 40 | 64 | 32 | 72 | 5,463 | 138 |
| mixed | 1 | 4 | 1024 | 50 | 1,024 | 29 | 73 | 21,792 | 1,044 |
| mixed | 1 | 4 | unbounded | 75 | 11,312 | 0 | 0 | 99,628 | 8,090 |
| mixed | 1 | 16 | 8 | 41 | 8 | 28 | 144 | 4,468 | 157 |
| mixed | 1 | 16 | 64 | 44 | 64 | 28 | 156 | 11,576 | 229 |
| mixed | 1 | 16 | 1024 | 55 | 1,024 | 27 | 161 | 27,623 | 1,172 |
| mixed | 1 | 16 | unbounded | 87 | 11,306 | 0 | 0 | 98,605 | 7,987 |
| mixed | 4 | 4 | 8 | 41 | 32 | 25 | 83 | 8,978 | 100 |
| mixed | 4 | 4 | 64 | 42 | 256 | 21 | 58 | 12,246 | 281 |
| mixed | 4 | 4 | 1024 | 43 | 4,096 | 19 | 56 | 49,767 | 3,832 |
| mixed | 4 | 4 | unbounded | 101 | 18,461 | 0 | 0 | 156,681 | 12,861 |
| mixed | 4 | 16 | 8 | 41 | 32 | 25 | 182 | 10,774 | 217 |
| mixed | 4 | 16 | 64 | 45 | 256 | 22 | 160 | 12,968 | 351 |
| mixed | 4 | 16 | 1024 | 68 | 4,096 | 19 | 130 | 65,928 | 4,016 |
| mixed | 4 | 16 | unbounded | 95 | 16,201 | 0 | 0 | 133,692 | 10,507 |

Only the unbounded queue changed memory and open handles. A worker stack reached the tree depth at most (36 in the mixed tree), and all stacks together stayed below 200 items.

### Production entry point with the result index

Thousand items per second / peak MiB.

| Tree | Lanes | Workers | 8 | 16 | 64 | 256 | 1024 | unbounded |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| wide | 1 | 1 | 506 / 218 | 505 / 218 | 524 / 218 | 508 / 218 | 521 / 217 | 422 / 202 |
| wide | 1 | 4 | 475 / 208 | 500 / 219 | 506 / 223 | 478 / 208 | 446 / 208 | 402 / 212 |
| wide | 1 | 16 | 169 / 298 | 176 / 301 | 179 / 300 | 179 / 300 | 180 / 300 | 170 / 300 |
| wide | 4 | 1 | 475 / 233 | 421 / 223 | 483 / 219 | 488 / 217 | 477 / 228 | 293 / 202 |
| wide | 4 | 4 | 188 / 232 | 351 / 233 | 323 / 226 | 288 / 232 | 213 / 235 | 171 / 218 |
| wide | 4 | 16 | 199 / 223 | 221 / 223 | 302 / 222 | 165 / 224 | 172 / 224 | 200 / 226 |
| deep | 1 | 1 | 6 / 238 | 7 / 237 | 6 / 238 | 6 / 238 | 6 / 237 | 6 / 239 |
| deep | 1 | 4 | 6 / 240 | 7 / 238 | 7 / 240 | 5 / 239 | 6 / 240 | 7 / 230 |
| deep | 1 | 16 | 7 / 243 | 7 / 241 | 7 / 242 | 7 / 244 | 6 / 244 | 6 / 241 |
| deep | 4 | 1 | 27 / 64 | 24 / 64 | 23 / 64 | 22 / 64 | 25 / 64 | 29 / 63 |
| deep | 4 | 4 | 28 / 67 | 27 / 68 | 25 / 67 | 29 / 68 | 25 / 67 | 28 / 67 |
| deep | 4 | 16 | 28 / 79 | 25 / 78 | 23 / 78 | 14 / 75 | 14 / 77 | 26 / 79 |
| mixed | 1 | 1 | 128 / 439 | 157 / 441 | 99 / 513 | 172 / 442 | 133 / 439 | 75 / 511 |
| mixed | 1 | 4 | 70 / 491 | 57 / 507 | 103 / 510 | 108 / 511 | 110 / 512 | 99 / 503 |
| mixed | 1 | 16 | 98 / 510 | 106 / 513 | 103 / 514 | 103 / 513 | 102 / 513 | 98 / 508 |
| mixed | 4 | 1 | 233 / 377 | 216 / 377 | 234 / 378 | 198 / 376 | 182 / 374 | 187 / 374 |
| mixed | 4 | 4 | 173 / 381 | 175 / 380 | 173 / 380 | 119 / 437 | 114 / 449 | 113 / 446 |
| mixed | 4 | 16 | 110 / 454 | 115 / 456 | 118 / 459 | 121 / 460 | 116 / 451 | 114 / 450 |

The publisher is the bottleneck, so the capacity has no visible effect. The process peak is mostly the result index: about 0.5 GiB for 257,000 mixed items and 0.24 GiB for 5,000 items in a chain of depth 1,000.

### Read delay

Thousand items per second (measured mean delay per read in µs). The Windows sleep turns 50 µs into about 0.5 ms and 1 ms into 1.5–12 ms depending on the run, so the Linux table below is the reference for latency. With one worker and 1 ms requested, only capacity 8 was measured on Windows (0.5 thousand items/s at a 12 ms effective delay); the other capacities would have taken over 40 minutes.

| Requested delay µs | Lanes | Workers | 8 | 16 | 64 | 256 | 1024 | unbounded |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 1 | 1 | 485 | 499 | 508 | 531 | 506 | 512 |
| 0 | 1 | 4 | 1,765 | 1,838 | 1,870 | 1,662 | 1,321 | 1,406 |
| 0 | 1 | 16 | 808 | 787 | 890 | 858 | 766 | 772 |
| 0 | 4 | 1 | 515 | 520 | 522 | 518 | 508 | 527 |
| 0 | 4 | 4 | 1,838 | 1,809 | 1,822 | 1,703 | 1,630 | 1,560 |
| 0 | 4 | 16 | 947 | 917 | 920 | 795 | 813 | 763 |
| 50 | 1 | 1 | 11.8 (531) | 11.8 (529) | 11.8 (530) | 11.9 (526) | 11.8 (531) | 11.6 (534) |
| 50 | 1 | 4 | 46.7 (527) | 47.9 (520) | 47.8 (522) | 48.1 (519) | 48.1 (519) | 47.9 (521) |
| 50 | 1 | 16 | 186.6 (496) | 188.0 (495) | 189.0 (498) | 187.6 (496) | 182.8 (493) | 181.9 (491) |
| 50 | 4 | 1 | 10.4 (576) | 8.8 (681) | 11.6 (531) | 11.7 (531) | 11.8 (525) | 11.7 (530) |
| 50 | 4 | 4 | 47.7 (518) | 47.6 (518) | 47.7 (517) | 47.4 (518) | 47.3 (519) | 47.2 (518) |
| 50 | 4 | 16 | 186.8 (498) | 187.4 (501) | 187.8 (492) | 188.5 (491) | 187.3 (485) | 184.0 (490) |
| 1000 | 1 | 1 | 0.5 (11978) | – | – | – | – | – |
| 1000 | 1 | 4 | 5.3 (4854) | 5.6 (4571) | 5.9 (4287) | 6.5 (3929) | 11.4 (2196) | 9.6 (2569) |
| 1000 | 1 | 16 | 67.8 (1476) | 68.4 (1456) | 68.7 (1449) | 67.9 (1465) | 67.2 (1463) | 68.0 (1453) |
| 1000 | 4 | 4 | 4.8 (5219) | 7.8 (3205) | 5.1 (4953) | 16.8 (1499) | 16.7 (1504) | 16.8 (1493) |
| 1000 | 4 | 16 | 67.0 (1478) | 67.6 (1458) | 66.6 (1471) | 68.0 (1435) | 67.7 (1439) | 67.4 (1454) |

With four workers and 1 ms requested, the rows differ mainly in the effective delay; time per read divided by workers is the same at every capacity.

### Read batches and many lanes

| Batch | Lanes | Workers | Capacity | Mode | Items | Thousand items/s | Peak MiB | Queue peak | Deepest stack | All stacks | Unrecorded entries peak | Open handles peak |
|---:|---:|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 256 | 1 | 4 | 8 | collector | 514,256 | 2,932 | 8 | 8 | 5 | 15 | 5,785 | 23 |
| 256 | 1 | 4 | 64 | collector | 514,256 | 2,659 | 12 | 64 | 3 | 12 | 18,188 | 65 |
| 256 | 1 | 4 | unbounded | collector | 514,256 | 2,059 | 99 | 1,920 | 0 | 0 | 360,448 | 1,410 |
| 256 | 1 | 16 | 8 | collector | 514,256 | 1,407 | 32 | 8 | 5 | 50 | 14,979 | 63 |
| 256 | 1 | 16 | 64 | collector | 514,256 | 1,567 | 13 | 64 | 3 | 39 | 22,380 | 106 |
| 256 | 1 | 16 | unbounded | collector | 514,256 | 1,053 | 127 | 1,918 | 0 | 0 | 360,282 | 1,411 |
| 256 | 1 | 4 | 64 | full | 514,256 | 222 | 701 | 64 | 3 | 12 | 24,746 | 65 |
| 256 | 4 | 4 | 8 | collector | 2,057,024 | 2,973 | 31 | 32 | 5 | 19 | 11,074 | 42 |
| 256 | 4 | 4 | 64 | collector | 2,057,024 | 2,978 | 47 | 256 | 3 | 12 | 60,493 | 232 |
| 256 | 4 | 4 | unbounded | collector | 2,057,024 | 2,103 | 402 | 7,680 | 0 | 0 | 1,436,176 | 5,633 |
| 256 | 4 | 16 | 8 | collector | 2,057,024 | 1,604 | 37 | 32 | 5 | 58 | 18,756 | 81 |
| 256 | 4 | 16 | 64 | collector | 2,057,024 | 1,409 | 44 | 256 | 3 | 43 | 56,068 | 232 |
| 256 | 4 | 16 | unbounded | collector | 2,057,024 | 1,180 | 304 | 7,174 | 0 | 0 | 1,019,948 | 4,226 |
| 256 | 16 | 4 | 8 | collector | 8,228,096 | 3,017 | 35 | 44 | 5 | 18 | 11,578 | 54 |
| 256 | 16 | 4 | 64 | collector | 8,228,096 | 2,766 | 48 | 268 | 3 | 12 | 60,905 | 244 |
| 256 | 16 | 4 | unbounded | collector | 8,228,096 | 2,342 | 402 | 7,692 | 0 | 0 | 1,434,332 | 5,645 |
| 256 | 16 | 16 | 8 | collector | 8,228,096 | 1,736 | 43 | 128 | 5 | 74 | 40,385 | 163 |
| 256 | 16 | 16 | 64 | collector | 8,228,096 | 1,452 | 86 | 1,024 | 3 | 45 | 236,007 | 928 |
| 256 | 16 | 16 | unbounded | collector | 8,228,096 | 1,200 | 1,500 | 30,719 | 0 | 0 | 5,114,851 | 22,529 |
| 8,192 | 1 | 4 | 8 | collector | 1,646,792 | 2,686 | 46 | 8 | 2 | 8 | 128,660 | 16 |
| 8,192 | 1 | 4 | 64 | collector | 1,646,792 | 2,434 | 148 | 64 | 2 | 8 | 490,604 | 60 |
| 8,192 | 1 | 4 | unbounded | collector | 1,646,792 | 1,943 | 297 | 192 | 0 | 0 | 1,048,072 | 129 |
| 8,192 | 1 | 16 | 8 | collector | 1,646,792 | 1,359 | 115 | 8 | 3 | 30 | 331,908 | 41 |
| 8,192 | 1 | 16 | 64 | collector | 1,646,792 | 1,425 | 198 | 64 | 2 | 20 | 584,979 | 87 |
| 8,192 | 1 | 16 | unbounded | collector | 1,646,792 | 1,169 | 297 | 197 | 0 | 0 | 1,046,962 | 130 |
| 8,192 | 1 | 4 | 64 | full | 1,646,792 | 171 | 2,043 | 64 | 2 | 8 | 496,991 | 60 |
| 8,192 | 4 | 4 | 8 | collector | 6,587,168 | 3,163 | 106 | 32 | 3 | 12 | 291,653 | 36 |
| 8,192 | 4 | 4 | 64 | collector | 6,587,168 | 2,799 | 500 | 256 | 2 | 8 | 1,867,323 | 228 |
| 8,192 | 4 | 4 | unbounded | collector | 6,587,168 | 2,711 | 1,084 | 768 | 0 | 0 | 4,160,984 | 513 |
| 8,192 | 4 | 16 | 8 | collector | 6,587,168 | 1,413 | 140 | 32 | 3 | 33 | 402,632 | 54 |
| 8,192 | 4 | 16 | 64 | collector | 6,587,168 | 1,488 | 403 | 256 | 2 | 30 | 1,315,094 | 182 |
| 8,192 | 4 | 16 | unbounded | collector | 6,587,168 | 1,205 | 817 | 765 | 0 | 0 | 2,891,743 | 385 |
| 8,192 | 16 | 4 | 8 | collector | 26,348,672 | 2,984 | 107 | 44 | 3 | 12 | 293,228 | 48 |
| 8,192 | 16 | 4 | 64 | collector | 26,348,672 | 3,015 | 506 | 268 | 2 | 8 | 1,867,534 | 240 |
| 8,192 | 16 | 4 | unbounded | collector | 26,348,672 | 2,778 | 1,084 | 780 | 0 | 0 | 4,164,477 | 525 |
| 8,192 | 16 | 16 | 8 | collector | 26,348,672 | 1,502 | 325 | 128 | 3 | 46 | 1,127,772 | 144 |
| 8,192 | 16 | 16 | 64 | collector | 26,348,672 | 1,554 | 1,918 | 1,024 | 2 | 29 | 7,435,499 | 912 |
| 8,192 | 16 | 16 | unbounded | collector | 26,348,672 | 1,291 | 4,256 | 3,071 | 0 | 0 | 15,859,642 | 2,049 |

### Large tree

| Mode | Directories | Items | Workers | Capacity | Seconds | Thousand items/s | Peak MiB | Queue peak | Deepest stack | All stacks | Unrecorded entries peak | Open handles peak |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| collector | 1,000,001 | 12,546,944 | 4 | 8 | 19.3 | 650 | 90 | 8 | 43 | 92 | 10,000 | 100 |
| collector | 1,000,001 | 12,546,944 | 4 | 64 | 18.9 | 663 | 91 | 64 | 42 | 95 | 11,094 | 154 |
| collector | 1,000,001 | 12,546,944 | 4 | 1024 | 17.9 | 700 | 101 | 1,024 | 40 | 86 | 32,828 | 1,100 |
| collector | 1,000,001 | 12,546,944 | 4 | unbounded | 27.3 | 459 | 505 | 113,932 | 0 | 0 | 1,000,686 | 80,143 |
| collector | 1,000,001 | 12,546,944 | 16 | 8 | 17.8 | 704 | 102 | 8 | 37 | 224 | 13,461 | 241 |
| collector | 1,000,001 | 12,546,944 | 16 | 64 | 18.1 | 693 | 102 | 64 | 33 | 227 | 15,196 | 297 |
| collector | 1,000,001 | 12,546,944 | 16 | 1024 | 13.9 | 903 | 107 | 1,024 | 34 | 219 | 33,648 | 1,250 |
| collector | 1,000,001 | 12,546,944 | 16 | unbounded | 20.6 | 608 | 527 | 113,925 | 0 | 0 | 1,000,386 | 80,131 |
| full | 100,001 | 1,258,623 | 4 | 8 | 33.1 | 38 | 2,867 | 8 | 35 | 80 | 10,643 | 91 |
| full | 100,001 | 1,258,623 | 4 | 64 | 26.8 | 47 | 2,880 | 64 | 31 | 73 | 11,385 | 140 |
| full | 100,001 | 1,258,623 | 4 | unbounded | 31.8 | 40 | 2,878 | 11,312 | 0 | 0 | 106,974 | 8,034 |
| full | 100,001 | 1,258,623 | 16 | 8 | 29.5 | 43 | 2,872 | 8 | 28 | 161 | 12,190 | 182 |
| full | 100,001 | 1,258,623 | 16 | 64 | 24.9 | 51 | 2,778 | 64 | 30 | 151 | 13,157 | 229 |
| full | 100,001 | 1,258,623 | 16 | unbounded | 29.1 | 43 | 2,907 | 11,297 | 0 | 0 | 106,144 | 7,991 |

## Linux results

### Read delay

| Requested delay µs | Lanes | Workers | 8 | 16 | 64 | 256 | 1024 | unbounded |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 1 | 1 | 142 | 96 | 110 | 120 | 101 | 114 |
| 0 | 1 | 4 | 197 | 189 | 67 | 282 | 266 | 240 |
| 0 | 1 | 16 | 174 | 238 | 192 | 278 | 289 | 291 |
| 0 | 4 | 1 | 130 | 111 | 131 | 126 | 143 | 125 |
| 0 | 4 | 4 | 291 | 56 | 327 | 292 | 302 | 195 |
| 0 | 4 | 16 | 262 | 48 | 298 | 300 | 340 | 200 |
| 50 | 1 | 1 | 42.8 (116) | 44.4 (115) | 44.8 (114) | 44.4 (114) | 45.0 (115) | 45.1 (115) |
| 50 | 1 | 4 | 127.6 (115) | 155.0 (121) | 101.2 (128) | 144.4 (122) | 155.8 (117) | 159.0 (117) |
| 50 | 1 | 16 | 273.8 (129) | 237.1 (128) | 231.2 (122) | 238.2 (127) | 245.4 (128) | 237.3 (129) |
| 50 | 4 | 1 | 22.3 (137) | 43.6 (114) | 44.7 (114) | 43.7 (115) | 43.8 (114) | 39.9 (114) |
| 50 | 4 | 4 | 139.9 (115) | 88.4 (130) | 107.5 (121) | 88.7 (126) | 107.1 (127) | 113.9 (123) |
| 50 | 4 | 16 | 318.2 (126) | 301.6 (130) | 248.7 (126) | 310.1 (125) | 228.0 (126) | 318.5 (127) |
| 1000 | 1 | 1 | 5.4 (1088) | 5.3 (1088) | 5.8 (1077) | 5.4 (1089) | 5.6 (1082) | 5.4 (1086) |
| 1000 | 1 | 4 | 21.7 (1079) | 21.8 (1077) | 23.2 (1072) | 22.0 (1076) | 22.3 (1078) | 22.5 (1076) |
| 1000 | 1 | 16 | 84.2 (1075) | 83.8 (1074) | 82.0 (1075) | 89.0 (1073) | 80.7 (1074) | 80.1 (1075) |
| 1000 | 4 | 1 | 5.4 (1085) | 5.6 (1082) | 5.5 (1088) | 5.7 (1084) | 5.8 (1080) | 5.8 (1077) |
| 1000 | 4 | 4 | 22.6 (1075) | 22.2 (1077) | 22.3 (1074) | 22.1 (1074) | 22.0 (1075) | 22.2 (1074) |
| 1000 | 4 | 16 | 84.2 (1074) | 86.4 (1073) | 84.4 (1074) | 83.0 (1077) | 87.0 (1076) | 83.9 (1074) |

With an accurate 1 ms delay the capacity has no effect on throughput, and 16 workers reach 15.5 times the single-worker rate even at capacity 8. Without delay and at 50 µs, the rows vary by up to a factor of five between neighbouring capacities, which matches the lock contention described above rather than any capacity trend.

### Read batches with four workers

8,192-entry batches and four workers. The 16-lane runs were stopped because one run would have taken several minutes.

| Lanes | Capacity | Mode | Items | Thousand items/s | Peak MiB | Queue peak | Deepest stack | All stacks | Unrecorded entries peak | Open handles peak |
|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 8 | collector | 1,646,792 | 123 | 58 | 8 | 2 | 8 | 136,575 | 16 |
| 1 | 64 | collector | 1,646,792 | 192 | 149 | 64 | 2 | 8 | 494,543 | 60 |
| 1 | unbounded | collector | 1,646,792 | 75 | 293 | 192 | 0 | 0 | 1,048,460 | 131 |
| 1 | 64 | full | 1,646,792 | 13 | 2,493 | 64 | 2 | 7 | 497,407 | 60 |
| 4 | 8 | collector | 6,587,168 | 104 | 104 | 32 | 3 | 12 | 300,364 | 36 |
| 4 | 64 | collector | 6,587,168 | 65 | 525 | 256 | 2 | 8 | 1,873,748 | 228 |
| 4 | unbounded | collector | 6,587,168 | 80 | 1,132 | 768 | 0 | 0 | 4,085,312 | 513 |

The held entries and memory follow the same bound as on Windows: four lanes at capacity 64 held 1.87 million entries, 89 % of 4 × 64 × 8,192.

### Collector throughput

Thousand items per second. The Linux main plan was stopped after 215 of 288 configurations because runs had become 2–40 times slower than on Windows. Missing cells were not measured.

| Tree | Lanes | Workers | 8 | 16 | 64 | 256 | 1024 | unbounded |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| wide | 1 | 1 | 74 | 62 | 118 | 92 | 119 | 32 |
| wide | 1 | 2 | 72 | 74 | 172 | 131 | 82 | 101 |
| wide | 1 | 4 | 79 | 99 | 94 | 130 | 212 | 170 |
| wide | 1 | 8 | 66 | 58 | 63 | 62 | 84 | 85 |
| wide | 1 | 16 | 23 | 25 | 26 | 28 | 26 | 27 |
| wide | 4 | 1 | 437 | 434 | 474 | 465 | 471 | 368 |
| wide | 4 | 2 | 597 | 494 | 610 | 482 | 727 | 311 |
| wide | 4 | 4 | 318 | 326 | 328 | 301 | 299 | 382 |
| wide | 4 | 8 | 300 | 292 | 304 | 301 | 260 | 351 |
| wide | 4 | 16 | 203 | 229 | 299 | 271 | 187 | 277 |
| deep | 1 | 1 | 4 | 4 | 4 | 4 | 4 | 4 |
| deep | 1 | 2 | 8 | 8 | 8 | 8 | 8 | 8 |
| deep | 1 | 4 | 15 | 13 | 13 | 12 | 12 | 12 |
| deep | 1 | 8 | 15 | 15 | 16 | 15 | 16 | 19 |
| deep | 1 | 16 | 19 | 20 | 20 | 20 | 20 | 20 |
| deep | 4 | 1 | 18 | 18 | 18 | 17 | 17 | 17 |
| deep | 4 | 2 | 37 | 38 | 38 | 38 | 39 | 38 |
| deep | 4 | 4 | 59 | 60 | 60 | 60 | 59 | 59 |
| deep | 4 | 8 | 66 | 68 | 67 | 67 | 66 | 65 |
| deep | 4 | 16 | 76 | 78 | 76 | 77 | 79 | 79 |
| mixed | 1 | 1 | 212 | 196 | 197 | 187 | 157 | 110 |
| mixed | 1 | 2 | 100 | 109 | 108 | 99 | 106 | 111 |
| mixed | 1 | 4 | 106 | 35 | 41 | 21 | 31 | 37 |
| mixed | 1 | 8 | 38 | 26 | 35 | 47 | 30 | – |

Production entry point (thousand items per second / peak MiB); the mixed full runs were not reached.

| Tree | Lanes | Workers | 8 | 16 | 64 | 256 | 1024 | unbounded |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| wide | 1 | 1 | 72 / 302 | 71 / 283 | 59 / 291 | 89 / 328 | 52 / 301 | 46 / 358 |
| wide | 1 | 4 | 45 / 301 | 68 / 302 | 18 / 311 | 65 / 309 | 59 / 303 | 31 / 339 |
| wide | 1 | 16 | 21 / 300 | 22 / 300 | 23 / 308 | 23 / 342 | 45 / 328 | 46 / 318 |
| wide | 4 | 1 | 322 / 240 | 295 / 240 | 327 / 240 | 339 / 239 | 308 / 237 | 216 / 241 |
| wide | 4 | 4 | 301 / 237 | 361 / 238 | 297 / 237 | 335 / 237 | 321 / 240 | 83 / 367 |
| wide | 4 | 16 | 231 / 229 | 226 / 228 | 231 / 227 | 203 / 229 | 186 / 230 | 123 / 324 |
| deep | 1 | 1 | 6 / 348 | 6 / 347 | 6 / 348 | 6 / 348 | 6 / 348 | 6 / 348 |
| deep | 1 | 4 | 6 / 306 | 6 / 301 | 6 / 299 | 6 / 328 | 6 / 300 | 6 / 299 |
| deep | 1 | 16 | 6 / 291 | 6 / 287 | 6 / 287 | 6 / 290 | 6 / 289 | 6 / 286 |
| deep | 4 | 1 | 22 / 98 | 23 / 94 | 22 / 98 | 22 / 98 | 23 / 99 | 23 / 98 |
| deep | 4 | 4 | 24 / 109 | 25 / 109 | 25 / 109 | 25 / 109 | 24 / 109 | 24 / 108 |
| deep | 4 | 16 | 23 / 101 | 22 / 101 | 22 / 101 | 23 / 101 | 24 / 103 | 24 / 99 |

In the 4,000-deep chain with 4 or 8 workers in one lane the Linux process peak reached 0.8–1.05 GiB, against 0.03–0.25 GiB on Windows. Up to 5,700 entries waited for the slower consumer, and each event carries a copy of its path, about 48 bytes per level with these names, so a few thousand events at depth 4,000 hold about a gigabyte. The channel bound is therefore 8,192 × (path + entry) bytes, about 1.5 GiB at depth 4,000 with short names.

## What a frame holds

Sizes from the measurement on x86_64: `Frame` 136 bytes, a lane queue or stack item `(u64, Frame)` 144 bytes, `ObservedEntry` 216 bytes and `TimedObservedEntry` 248 bytes. A queued or stacked frame holds:

- its path, one `Vec<u8>` per component: 24 bytes for the outer vector plus 24 bytes and the name bytes per level (Windows names are UTF-16LE, at most 510 bytes);
- a pending child: the observed entry in a box (216 bytes plus its name) and a reference to its parent's handle, which keeps that handle open until the child is opened;
- an opened directory: its own handle, shared with its pending children, and the entries of its last read. `frame.entries = batch.into()` reuses the adapter's vector, and taking entries from the front never shrinks it, so the frame keeps the memory of a whole batch, up to 8,192 × (248 bytes + name), until the next read replaces it or the directory ends. This includes the turns in which all entries are used and the frame waits in the queue for its next read;
- a root frame before its first result: also the entered subdirectories of its first batch, at most one more batch.

Each open handle also owns adapter state outside the collector. The Windows adapter buffers the records of one 64 KiB directory read and the Linux adapter up to 256 entries.

With `L` lanes, capacity `C`, `W` workers, tree depth `D`, read batch `B` (at most 8,192; 256 in every adapter) and name size `N`, the collector holds at most:

- lane queues: `L × C` items, plus the scope roots queued before collection;
- worker stacks: a stack follows one depth-first chain, and the lane bound test asserts at most `W × (D + 2)` stacked items. The measured maxima were the tree depth for one stack and 227 items for all stacks;
- in progress: per worker, one claimed item and at most one found child;
- open handles: at most twice the queued, stacked and claimed items, plus the roots;
- returned entries: at most `(L × C + W × (D + 4) + roots) × B`, with one more batch per root;
- the observation channel: 8,192 events, each with a path copy and a boxed entry; at depth `D` that is up to `8,192 × (24 + D × (24 + N) + 248 + N)` bytes.

The bytes per held item are at most `144 + 24 + D × (24 + N) + max(216 + N, B × (248 + N))`. With `B = 8,192` and short names that is about 2.1 MiB, so 16 lanes at capacity 64 can hold `16 × 64 × 2.1 MiB ≈ 2.1 GiB` in the queues alone; the measured process peak of 1.9 GiB came close. With `B = 256` an item holds at most about 66 KiB and the same queues about 66 MiB. The stack term matters only when deep chains also fill whole batches.

## Possible reductions

These were not implemented.

1. Release the batch buffer once its entries are used: replace `frame.entries` with an empty `VecDeque` before requeueing a frame that waits for its next read. Waiting frames then hold only their path and handle. Frames with unused entries still hold their batch.
2. Keep frames with unused entries out of the lane queue: return them to the finding worker's stack, where other workers of the lane can still steal them, and queue only pending children and frames waiting for a read. The lane queue term becomes `L × C × (about 0.4 KiB + path)`, independent of the batch size. The batch-holding frames are then bounded by the stacks, `W × (D + 2)`. This changes when a prioritized area takes effect for an opened directory (FR8.HD1.app-host.DD1), which needs a design review.
3. Lower the largest accepted batch from 8,192 to the adapters' 256. The per-item bound falls by a factor of 32. The 8,192 figure belongs to the stage channel in the F02 DLD, so this also needs the design owner.
4. Count the capacity in held entries or bytes instead of directories.

## Not measured

- Real storage: no directory system calls were made. The platform adapters were not used, because `scan-engine` may not depend on `platform-os`, and the DiskUnfold executable was not started. Slow disks, network drives, cloud placeholders, cold caches and variable per-directory latency were not measured; the fixed sleep only approximates latency.
- macOS and ARM64; native Linux outside Docker on WSL2.
- Lanes with more than 16 workers, up to `MAX_WORKERS` (64).
- The Linux main plan after 215 configurations, the Linux large plan, and the Linux batch plan with 16 workers.
- Priority areas and cancellation during collection, long names and deep trees whose directories all fill full batches.
- The split of collector memory between queues, stacks, channel and handles, and the split of full-entry memory between the result index and partial publications.
- The order in which results appear for different capacities.
