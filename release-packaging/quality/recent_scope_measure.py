"""Measure cec7250 recent-scope SQL on a disposable SQLite database.

Run from any directory with Python 3: python release-packaging/quality/recent_scope_measure.py
Synthetic bodies are not a published scope format. Results exclude IPC and validation.
"""

import json
from pathlib import Path
import sqlite3
import tempfile
import time


SCHEMA = Path(__file__).resolve().parents[2] / "app-db/src/recent_scope_schema.sql"
INSERT = """INSERT INTO user_env_recent_scope VALUES(?,?,1,?,1,?,?,?,1)
ON CONFLICT(scope_fingerprint,fingerprint_format) DO UPDATE SET
last_used_at_ms=excluded.last_used_at_ms,updated_at_ms=excluded.updated_at_ms,
row_version=row_version+1"""
EVICT = """DELETE FROM user_env_recent_scope WHERE recent_scope_id IN
(SELECT recent_scope_id FROM user_env_recent_scope
 ORDER BY last_used_at_ms DESC,recent_scope_id ASC LIMIT -1 OFFSET ?)"""
LIST = """SELECT recent_scope_id,scope_fingerprint,fingerprint_format,scope_body,
include_count,last_used_at_ms,created_at_ms,updated_at_ms,row_version
FROM user_env_recent_scope ORDER BY last_used_at_ms DESC,recent_scope_id ASC"""


def identifier(number, length):
    return number.to_bytes(length, "big")


def percentile(samples):
    return sorted(samples)[(95 * len(samples) + 99) // 100 - 1] / 1_000_000


def measure(cap, body_size):
    overhead = len(json.dumps({"synthetic": ""}, separators=(",", ":")))
    body = json.dumps({"synthetic": "x" * (body_size - overhead)}, separators=(",", ":"))
    assert len(body.encode()) == body_size
    with tempfile.TemporaryDirectory(prefix="du-recent-measure-") as directory:
        db = sqlite3.connect(Path(directory) / "test.db", isolation_level=None)
        db.execute("PRAGMA journal_mode=WAL")
        db.execute("PRAGMA synchronous=NORMAL")
        db.executescript(SCHEMA.read_text(encoding="utf-8"))
        for i in range(cap):
            db.execute(INSERT, (identifier(i, 16), identifier(i, 32), body, i, i, i))
        reads, writes, updates = [], [], []
        for i in range(220):
            start = time.perf_counter_ns()
            rows = db.execute(LIST).fetchall()
            # Model adapter JSON parsing; the owner validator and IPC are unavailable.
            for row in rows:
                json.loads(row[3])
            elapsed = time.perf_counter_ns() - start
            assert len(rows) == cap
            if i >= 20:
                reads.append(elapsed)
            existing = cap - 1 if i == 0 else cap + i - 1
            start = time.perf_counter_ns()
            db.execute("BEGIN IMMEDIATE")
            db.execute(INSERT, (identifier(cap + 220 + i, 16), identifier(existing, 32), body, cap + i, cap + i, cap + i))
            db.execute(EVICT, (cap,))
            db.execute("COMMIT")
            if i >= 20:
                updates.append(time.perf_counter_ns() - start)
            number = cap + i
            start = time.perf_counter_ns()
            db.execute("BEGIN IMMEDIATE")
            db.execute(INSERT, (identifier(number, 16), identifier(number, 32), body, number, number, number))
            db.execute(EVICT, (cap,))
            db.execute("COMMIT")
            elapsed = time.perf_counter_ns() - start
            if i >= 20:
                writes.append(elapsed)
        print(f"cap={cap} body_bytes={body_size} list_p95_ms={percentile(reads):.3f} "
              f"update_p95_ms={percentile(updates):.3f} write_evict_p95_ms={percentile(writes):.3f} "
              f"listed_body_bytes={sum(len(row[3].encode()) for row in rows)} "
              f"db_bytes={db.execute('PRAGMA page_count').fetchone()[0] * db.execute('PRAGMA page_size').fetchone()[0]}")
        db.close()


if __name__ == "__main__":
    print(f"SQLite {sqlite3.sqlite_version}; 20 warmups + 200 samples; WAL/NORMAL; isolated temporary database")
    for size in (512, 4096):
        for limit in (50, 100, 500):
            measure(limit, size)
