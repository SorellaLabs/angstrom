INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    is_leader,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET is_leader = EXCLUDED.is_leader,
    last_updated_at = NOW()
