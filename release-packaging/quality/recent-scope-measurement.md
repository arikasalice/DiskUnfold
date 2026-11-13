# F01 recent-scope SQL measurement

Run `python release-packaging/quality/recent_scope_measure.py` from the repository
root. The script creates and removes an isolated temporary database. It reproduces
the SQL shape of cec7250; that commit is not present in the 8df5277 baseline.
The body is synthetic JSON, not an approved scope format. List timings include
fetching full rows and Python JSON parsing, not Rust validation or IPC serialization.
The write timing includes a transaction and oldest-row eviction; update timing
uses an existing fingerprint. There are 20 warmups and 200 timed samples, with
p95 calculated using nearest rank. SQLite uses WAL and synchronous NORMAL.

Windows 11 Pro, Intel Core i7-13700KF, Python 3.14.6, SQLite 3.50.4, one run:

| Rows | Body bytes each | List p95 ms | Update p95 ms | Insert and evict p95 ms | Listed body bytes | Database page bytes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 512 | 0.135 | 0.080 | 0.143 | 25,600 | 57,344 |
| 100 | 512 | 0.276 | 0.082 | 0.143 | 51,200 | 94,208 |
| 500 | 512 | 1.218 | 0.178 | 0.260 | 256,000 | 368,640 |
| 50 | 4096 | 0.312 | 0.107 | 0.148 | 204,800 | 262,144 |
| 100 | 4096 | 0.620 | 0.122 | 0.166 | 409,600 | 499,712 |
| 500 | 4096 | 4.168 | 0.327 | 0.299 | 2,048,000 | 2,383,872 |

Page bytes are SQLite page count times page size, excluding the separate WAL and
shared-memory files; they are not total disk consumption. Listed body bytes are
not an IPC response size: IF-03 returns summaries and has not been connected.
The run is warm, single-process, uncontended and on one machine. It does not
establish performance for slow disks, large real scopes, concurrent writers,
startup, or the real application. It uses Python SQLite, not the Rust adapter.

100 rows is a candidate for further product review, not an approved limit: it
keeps this sample's list body load below 500 KB at 4 KiB per synthetic row, while
500 rows exceeds 2 MB. No published body-size bound makes that an upper bound.
F01 DLD 97 Q8 says to decide after measurement but specifies no numeric target or
acceptance threshold there. HLD 110 assigns the cap to architecture DLD settings.
Approval requires real scope-size distributions, supported-platform end-to-end
latency and storage measurements, and a product decision.
