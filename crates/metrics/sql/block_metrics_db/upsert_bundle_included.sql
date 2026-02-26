INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    bundle_included,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET bundle_included = EXCLUDED.bundle_included,
    last_updated_at = NOW()
