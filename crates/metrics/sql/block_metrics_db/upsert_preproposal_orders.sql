INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    preproposal_limit_orders,
    preproposal_searcher_orders,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET preproposal_limit_orders = EXCLUDED.preproposal_limit_orders,
    preproposal_searcher_orders = EXCLUDED.preproposal_searcher_orders,
    last_updated_at = NOW()
