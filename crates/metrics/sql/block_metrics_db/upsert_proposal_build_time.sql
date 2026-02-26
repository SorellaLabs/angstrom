INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    proposal_build_time_ms,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET proposal_build_time_ms = EXCLUDED.proposal_build_time_ms,
    last_updated_at = NOW()
