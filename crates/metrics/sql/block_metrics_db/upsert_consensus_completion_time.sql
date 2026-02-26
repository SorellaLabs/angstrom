INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    consensus_completion_time_ms,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET consensus_completion_time_ms = EXCLUDED.consensus_completion_time_ms,
    last_updated_at = NOW()
