WITH selected AS MATERIALIZED (
    SELECT item_seq, location_text, outcome, reason_key, review_needed, held,
           outcome_unknown, observed_size, observed_size_state, displaced_to_trash,
           displaced_name, displaced_original_location, displaced_trashed_at_ms
    FROM job_runtime_item_result
    WHERE job_id = ?1 AND item_seq <= ?2 AND (?3 IS NULL OR item_seq > ?3)
      AND (?4 IS NULL OR outcome = ?4)
    ORDER BY item_seq
    LIMIT ?5
), duplicates AS MATERIALIZED (
    SELECT displaced_name, displaced_original_location, displaced_trashed_at_ms
    FROM job_runtime_item_result
    WHERE job_id = ?1 AND item_seq <= ?2 AND displaced_to_trash = 1
      AND displaced_name IS NOT NULL AND displaced_original_location IS NOT NULL
      AND displaced_trashed_at_ms IS NOT NULL
      AND EXISTS (SELECT 1 FROM selected WHERE displaced_to_trash = 1)
    GROUP BY displaced_name, displaced_original_location, displaced_trashed_at_ms
    HAVING count(*) > 1
)
SELECT item.item_seq, item.location_text, item.outcome, item.reason_key,
       item.review_needed, item.held, item.outcome_unknown, item.observed_size,
       item.observed_size_state, item.displaced_to_trash, item.displaced_name,
       item.displaced_original_location, item.displaced_trashed_at_ms,
       duplicates.displaced_name IS NOT NULL
FROM selected AS item
LEFT JOIN duplicates ON item.displaced_to_trash = 1
    AND duplicates.displaced_name = item.displaced_name
    AND duplicates.displaced_original_location = item.displaced_original_location
    AND duplicates.displaced_trashed_at_ms = item.displaced_trashed_at_ms
ORDER BY item.item_seq
