INSERT INTO ang_block_state_metrics (
    chain_id,
    block_number,
    node_address,
    state,
    slot_offset_ms,
    limit_orders,
    searcher_orders,
    updated_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
ON CONFLICT (chain_id, block_number, node_address, state) DO UPDATE
SET slot_offset_ms = EXCLUDED.slot_offset_ms,
    limit_orders = EXCLUDED.limit_orders,
    searcher_orders = EXCLUDED.searcher_orders,
    updated_at = NOW()
