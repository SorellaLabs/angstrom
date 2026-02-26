-- Reference schema for Angstrom block metrics storage.
-- Do not execute from Angstrom runtime. Apply via deployment migrations.

CREATE TABLE IF NOT EXISTS ang_block_metrics (
    chain_id BIGINT NOT NULL,
    block_number BIGINT NOT NULL,
    node_address TEXT NOT NULL,
    preproposal_limit_orders BIGINT,
    preproposal_searcher_orders BIGINT,
    preproposals_collected BIGINT,
    is_leader BOOLEAN,
    matching_input_limit_pre_quorum BIGINT,
    matching_input_searcher_pre_quorum BIGINT,
    matching_input_limit_post_quorum BIGINT,
    matching_input_searcher_post_quorum BIGINT,
    matching_pools_solved BIGINT,
    matching_orders_filled BIGINT,
    matching_orders_partial BIGINT,
    matching_orders_unfilled BIGINT,
    matching_orders_killed BIGINT,
    bundle_generated BOOLEAN,
    submission_started_slot_offset_ms BIGINT,
    submission_completed_slot_offset_ms BIGINT,
    submission_latency_ms BIGINT,
    submission_success BOOLEAN,
    bundle_included BOOLEAN,
    proposal_build_time_ms BIGINT,
    proposal_verification_time_ms BIGINT,
    consensus_completion_time_ms BIGINT,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (chain_id, block_number, node_address)
);

CREATE TABLE IF NOT EXISTS ang_block_state_metrics (
    chain_id BIGINT NOT NULL,
    block_number BIGINT NOT NULL,
    node_address TEXT NOT NULL,
    state TEXT NOT NULL,
    slot_offset_ms BIGINT NOT NULL,
    limit_orders BIGINT NOT NULL,
    searcher_orders BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (chain_id, block_number, node_address, state),
    CONSTRAINT chk_ang_block_state_metrics_state CHECK (
        state IN ('BidAggregation', 'PreProposal', 'PreProposalAggregation', 'Proposal', 'Finalization')
    )
);

CREATE TABLE IF NOT EXISTS ang_block_submission_endpoint_metrics (
    id BIGSERIAL PRIMARY KEY,
    chain_id BIGINT NOT NULL,
    block_number BIGINT NOT NULL,
    node_address TEXT NOT NULL,
    submitter_type TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    success BOOLEAN NOT NULL,
    latency_ms BIGINT NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ang_block_metrics_node_block
    ON ang_block_metrics (chain_id, node_address, block_number DESC);

CREATE INDEX IF NOT EXISTS idx_ang_block_metrics_block_node
    ON ang_block_metrics (block_number, node_address);

CREATE INDEX IF NOT EXISTS idx_ang_block_metrics_updated_at
    ON ang_block_metrics (last_updated_at);

CREATE INDEX IF NOT EXISTS idx_ang_block_state_metrics_block_node_state
    ON ang_block_state_metrics (block_number, node_address, state);

CREATE INDEX IF NOT EXISTS idx_ang_block_state_metrics_updated_at
    ON ang_block_state_metrics (updated_at);

CREATE INDEX IF NOT EXISTS idx_ang_block_submission_block_node
    ON ang_block_submission_endpoint_metrics (chain_id, block_number, node_address);

CREATE INDEX IF NOT EXISTS idx_ang_block_submission_block_node_submitter_endpoint
    ON ang_block_submission_endpoint_metrics (block_number, node_address, submitter_type, endpoint);

CREATE INDEX IF NOT EXISTS idx_ang_block_submission_observed_at
    ON ang_block_submission_endpoint_metrics (observed_at);
