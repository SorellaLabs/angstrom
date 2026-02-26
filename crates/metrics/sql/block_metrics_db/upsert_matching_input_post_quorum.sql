INSERT INTO ang_block_metrics (
    chain_id,
    block_number,
    node_address,
    matching_input_limit_post_quorum,
    matching_input_searcher_post_quorum,
    first_seen_at,
    last_updated_at
) VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
ON CONFLICT (chain_id, block_number, node_address) DO UPDATE
SET matching_input_limit_post_quorum = EXCLUDED.matching_input_limit_post_quorum,
    matching_input_searcher_post_quorum = EXCLUDED.matching_input_searcher_post_quorum,
    last_updated_at = NOW()
