WITH entries AS (
 SELECT link.job_id, link.reckon_at_ms, link.record_seq,
        COALESCE(record.kind_id, started.kind_id) AS kind_id,
        COALESCE(record.kind_version, started.kind_version) AS kind_version,
        COALESCE(record.final_state,
                 CASE WHEN interrupted.job_id IS NOT NULL THEN 'interrupted' END) AS final_state,
        COALESCE(record.scope_summary, started.scope_summary) AS scope_summary,
        record.completed_count, record.overwritten_count, record.failed_count,
        record.skipped_count, record.unprocessed_count,
        COALESCE(record.started_at_ms, started.started_at_ms) AS started_at_ms,
        record.finished_at_ms, link.predecessor_job_id, link.relation_kind
 FROM job_runtime_job_link AS link
 LEFT JOIN job_runtime_job_record AS record USING (job_id)
 LEFT JOIN job_runtime_job_interrupt AS interrupted USING (job_id)
 LEFT JOIN job_runtime_job_start AS started ON interrupted.job_id = started.job_id
 WHERE link.record_seq <= ?1
   AND (?2 IS NULL OR link.reckon_at_ms >= ?2)
   AND (?3 IS NULL OR link.reckon_at_ms < ?3)
   AND (?4 IS NULL OR link.reckon_at_ms < ?4
        OR (link.reckon_at_ms = ?4 AND link.record_seq < ?5))
)
SELECT job_id, reckon_at_ms, record_seq, kind_id, kind_version, final_state, scope_summary,
       completed_count, overwritten_count, failed_count, skipped_count, unprocessed_count,
       started_at_ms, finished_at_ms, predecessor_job_id
FROM entries
WHERE (?6 = '[]' OR kind_id IN (SELECT value FROM json_each(?6)))
  AND (?7 IS NULL OR final_state = ?7)
  AND (?8 IS NULL OR relation_kind = ?8)
ORDER BY reckon_at_ms DESC, record_seq DESC
LIMIT ?9;
