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
