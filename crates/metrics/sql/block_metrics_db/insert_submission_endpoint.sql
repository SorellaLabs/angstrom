INSERT INTO ang_block_submission_endpoint_metrics (
    chain_id,
    block_number,
    node_address,
    submitter_type,
    endpoint,
    success,
    latency_ms,
    observed_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
