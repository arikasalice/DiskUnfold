# F01 recent-source admission boundary: evidence and proposed integration

Base: `d9e8fb6`. This is a proposal and test evidence, not a production recent
source, database write, new IPC field, handle provider or application wiring.
F01 HLD 111 IF-06 and DLD 102 section 2.3.3 assign immutable recent-source
binding to engine-api and persistence to app-host/user-env after a successful
GUI receipt. Source failure does not undo an admitted scan. A reused active job
must retain its first source; incomplete evidence yields no source.

## Current path and ownership

`scope::verify` reopens and checks roots and a review fingerprint; the exclusion
case owns `BoundScopeProof`, while the plain path resolves a digest and retains
only roots. `StartScan::execute` moves these verified `OpenedScopeRoot` values
into `MetadataScan`, and `StartScan::admit` serializes submission and the cached
receipt with a mutex. Created stores the receipt; Existing looks up the first
receipt and returns its job, scope version, lineage and fingerprint. The
engine entry test instruments synthetic root instances: collection uses the
Created request's directory instance twice, not the later Existing request's
instance. It also verifies the receipt fields match. This does not show that a
recent-source object exists: no such object or app-host handoff is implemented.

`ScopeRootPort::open_root` returns a boxed directory owned by the scan. Neither
that port nor `AnchoredDirectory` exposes an optional same-handle recent probe.
Windows `open_with_recent_probe` duplicates during a separate open call and is
not used by normal admission; opening a second time after admission cannot
establish it is the scan handle. Its synchronous `File::try_clone` can fail;
its subsequent Windows observations may block and are not forcibly cancelled.
A shared path, volume label or matching integer supplied by a caller is not a
substitute for same-handle provenance. No source can currently be safely
extracted from the production path within the assigned file/contract boundary.

## Proposed narrow boundary

1. At the composition boundary, extend the owner-approved root observation port
   with an **optional**, bounded-cost capability that returns scan ownership and
   an opaque deferred observer from the same initial handle. The adapter owns
   the construction and provenance; unsupported, failure or incomplete data is
   `None` and never prevents the scan. Do not reopen by name. Avoid assuming
   synchronous handle duplication has a hard deadline; demonstrate an acceptable
   admission latency and design isolation first.
2. Keep `VerifiedScope`'s immutable normalized conditions, fingerprint format
   and exact fingerprint together with an all-or-nothing observer set for the
   retained roots. The exclusion branch must expose its original-to-comparison
   key provenance from `BoundScopeProof`; until that owner contract exists,
   report no source for exclusions. Unsupported comparisons, incomplete roots,
   unsupported origin or mounts likewise produce no source, with scan behavior
   unchanged. Do not expose a constructor taking caller-set verification flags.
3. During the existing admission lock, install an opaque candidate only for
   `Created`, bound to job ID, scope version and lineage from the **same**
   accepted submission. `Existing` reuses that exact first candidate, never the
   newly revalidated request. Failed submission or stale review drops candidate
   and handles; a queue-full result publishes none. Keep cancellation and job
   state rechecks at the deferred observer and publication boundary. A bounded
   queue must fail without blocking the scan; enforce at-most-once/lifetime
   rules and specify shutdown, terminal jobs and cross-channel access before use.
4. Hand off only to the local app-host after the GUI receipt, with no public IPC
   field. App-host/user-env owns remember-setting serialization and eventual
   persistence. Work by CLI/MCP cannot itself produce a write; the still-open
   ownership and authorization semantics for an Existing job created by another
   origin need a decision before enabling any cross-origin reuse. A late optional
   failure cannot change the receipt or cancel the scan. Handle duplication or
   worker cancellation does not undo a previously committed write.

The existing `Admitted` stores just one active receipt and has no handle-source
or internal receipt API. Implementing the above without a real owner port and
lifecycle contract would create an unverified interface, so this change only
strengthens test evidence and specifies the missing boundary.

## Windows evidence and limits

Windows 11, local temporary NTFS fixtures: existing `recent_probe` tests passed
for same-handle observation, replacement detection, cancellation/injected failure,
and a 600-entry multi-batch collection while the duplicate was observed. These
are adapter tests, not a wired `scan.start` execution.

A separate same-process `DuplicateHandle(DUPLICATE_SAME_ACCESS)` microprobe used
one already-open temporary directory handle, 100 warmups and 2,000 timed calls;
all succeeded: p50 0.200 us, p95 0.300 us, p99 1.800 us on this machine. Timing
covers the Windows call only; it excludes open, Rust `File::try_clone` wrapper,
worker dispatch, observation, close, contention, security products and network
or other filesystem conditions. It is not an end-to-end admission measurement
or worst-case bound and cannot justify putting unbounded work on `scan.start`.

Remaining tests must exercise a real normal engine composition, Created and
Existing handoff including different sources, all-or-nothing multi-root failure,
queue pressure, cancellation races, unchanged scan success after duplication or
observation failure, and Windows/Linux/macOS handle behavior. No user DB or app
was opened in this investigation.
