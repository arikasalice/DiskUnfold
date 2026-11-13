# Q11: v1 fixture size measurements

These are synthetic, in-memory fixtures run on Windows at base `4e98c6f`.
They use the existing v1 Volume and Item encoders, the existing test's zstd
stream encoder (level 1, checksum on), the test envelope construction, and
the existing sealed-envelope and semantic validation. There is no production
snapshot writer at this base. They are not measurements of an actual scan,
filesystem allocation, atomic replacement, or catalog transaction. The two
root links are synthetically generated, not observed Windows volumes. Item
path ordering is not established by this semantic validator.

Run `cargo test --locked -p du-snapshot-store q11_fixture_measurements -- --nocapture`.
The pseudo-random name stream uses xorshift64 with seed `0x5eed123456789abc`.
Native zstd reports version 1.5.7; fixture names use the specified byte count
before UTF-16 conversion. Counts include two root items. Results in bytes:

| Case | Items | Metadata | Record stream | zstd frame | Whole envelope |
| --- | ---: | ---: | ---: | ---: | ---: |
| small | 2 | 600 | 137 | 80 | 752 |
| repeated 64-byte names | 10,002 | 600 | 980,137 | 15,552 | 16,224 |
| seeded 64-byte ASCII names | 10,002 | 600 | 980,137 | 520,756 | 521,428 |
| repeated 1,023-byte UTF-8 names | 102 | 600 | 105,837 | 398 | 1,070 |
| seeded ASCII names encoded UTF-16LE | 10,002 | 615 | 1,620,139 | 712,102 | 712,789 |
| unique siblings with shared prefix | 10,002 | 600 | 560,137 | 26,671 | 27,343 |
| unique siblings with shared prefix, million | 1,000,002 | 600 | 56,000,137 | 2,843,339 | 2,844,011 |

The repeated-name cases include duplicate sibling names and are compression
stress vectors, not legal directory listings. The unique-siblings case uses
`common-prefix-` plus an eight-digit unique number and alternates two parents;
it has no duplicate names per parent. This still does not establish path order.

## Counting compression before writing

For every fixture, a count-only sink and a `Vec` sink receive independent
zstd encoders with the same level, checksum, exact input bytes and schedule:
8,192-byte chunks, either no explicit flush or flush after every four chunks,
then finish. Within each schedule the count matches the Vec length, and two
independent Vec outputs match byte-for-byte. An injected count-sink failure propagates. This demonstrates equality
for these fixtures and library version, not all possible producers or versions.
The two schedules themselves need not match: for the unique case they produced
26,671 and 25,832 bytes respectively. Stable input, encoder settings, library,
chunk/flush sequence, and successful finish would all need to be pinned across
both passes; a mismatch must fail before publication.

One Windows debug run, single observation, measured count-only / Vec time
in microseconds as follows: repeated 2,471/2,446, random 19,257/19,257,
UTF-16 random 35,123/36,629, unique 2,950/2,922 (no explicit flush).
This excludes generating the record stream, disk I/O, reservation, DB work,
and any production pipeline. It is not an NFR or performance acceptance run.
The preflight does a second compression pass; the pinned source must survive
both passes without changing. F07 HLD 116 refers GUI response to F02 NFR4–6
and specifies returning the job reference promptly, so where preflight runs
relative to admission and job submission is a design decision, not settled
by this measurement. Published F07 DLD body remains unavailable to this run.

A later single Windows debug observation of the million-name vector measured
292,315 microseconds for count-only and 302,407 microseconds for Vec output
without explicit flush (316,312 and 325,179 microseconds with periodic flush).
This starts with already serialized in-memory records and is neither a full
admission timing nor a percentile. The fixture is not a pinned scan result.
Run this large fixture explicitly with `cargo test --locked -p du-snapshot-store
q11_million_measurement -- --ignored --nocapture`.

The bundled upstream zstd 1.5.7 `zstd.h` documents `ZSTD_compressBound` for
one-pass compression, not a general bound for the selected streaming and
explicit-flush schedule. A one-pass alternative requires its own validation
of memory, frame format, checksum settings, inputs and maximum size; it is not
the present streaming pipeline and is not selected as an implementation here.

The output sink here is memory, not an actual file. We have not measured
available filesystem bytes or allocation costs, performed a pre-write free
space check, or tested concurrent ENOSPC and atomic preservation. Those are
separate prerequisites before treating exact compressed bytes as a safe space
admission value. This fixture does not implement or change a public contract.

The fixture checks sealed validation and byte-for-byte recovered records,
rejects a changed envelope hash, rejects a name containing a separator, and
checks the current byte-name codec accepts 1,024 bytes and rejects 1,025.
An initial 6,000-byte UTF-8 name failed with `InvalidName`; the final long
case uses 1,023 bytes. No failed input is counted as a valid envelope.

For these envelopes, exact file bytes are `16 + metadata_bytes + frame_bytes
+ 56`, including the BLAKE3 hash. Exact record bytes can be counted only
after encoding all actual Volume, Item and Gap fields and framing, including
the actual name encoding and optional values. The compressed size is known
after compression. These figures do not establish a worst-case bound for the
streaming encoder, free disk space, filesystem allocation overhead, concurrent
writers, or a safety margin. In particular, compression ratios observed here
cannot justify a pre-write estimate or multiplier. Q11 still needs a verified
streaming bound and resource accounting or another explicitly approved
decision, with insufficient/unknown-space behavior and boundary tests.
