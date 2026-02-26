INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    submission_completed_slot_offset_ms,
    submission_latency_ms,
    submission_success,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET submission_completed_slot_offset_ms = EXCLUDED.submission_completed_slot_offset_ms,
    submission_latency_ms = EXCLUDED.submission_latency_ms,
    submission_success = EXCLUDED.submission_success,
    last_updated_at = NOW()
