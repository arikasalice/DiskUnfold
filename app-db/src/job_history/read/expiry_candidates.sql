SELECT link.job_id, link.reckon_at_ms
FROM job_runtime_job_link AS link INDEXED BY ix_job_runtime_job_link_reckon
LEFT JOIN job_runtime_job_record AS record USING (job_id)
LEFT JOIN job_runtime_job_interrupt AS interrupted USING (job_id)
WHERE link.reckon_at_ms <= ?1
  AND link.reckon_at_ms <= ?2
  AND (record.job_id IS NOT NULL OR interrupted.job_id IS NOT NULL)
ORDER BY link.job_id ASC;
