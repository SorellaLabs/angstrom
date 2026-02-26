INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    matching_pools_solved,
    matching_orders_filled,
    matching_orders_partial,
    matching_orders_unfilled,
    matching_orders_killed,
    bundle_generated,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET matching_pools_solved = EXCLUDED.matching_pools_solved,
    matching_orders_filled = EXCLUDED.matching_orders_filled,
    matching_orders_partial = EXCLUDED.matching_orders_partial,
    matching_orders_unfilled = EXCLUDED.matching_orders_unfilled,
    matching_orders_killed = EXCLUDED.matching_orders_killed,
    bundle_generated = EXCLUDED.bundle_generated,
    last_updated_at = NOW()
