# Angstrom Grafana Dashboards

## Setup

### Data Source Configuration

The dashboards expect a Prometheus data source with UID `angstrom-prometheus`.

To configure:
1. Go to Grafana → Connections → Data Sources → Add data source
2. Select Prometheus
3. Set the UID to `angstrom-prometheus` (in the data source settings JSON or via API)
4. Or update the dashboard JSON to match your existing data source UID

### Import Dashboard

1. Go to Grafana → Dashboards → Import
2. Upload `block-analysis.json` or paste its contents
3. Select your Prometheus data source if prompted

## Dashboards

### Block Analysis (`block-analysis.json`)

Analyze consensus and submission metrics for a specific block across all nodes.

**Usage:**
1. Enter a block number in the "Block Number" text box at the top
2. Optionally filter by specific node(s) using the "Node ID" dropdown
3. View the populated tables

**Sections:**

#### Consensus Metrics
- **State Timing Table**: Shows slot offset and order counts at each consensus state (PreProposal, PreProposalAggregation, Proposal, Finalization)
- **Matching Results Table**: Shows matching engine inputs (pre/post quorum) and outcomes (filled/partial/unfilled/killed orders)
- **General Consensus Table**: Shows preproposal counts, leader status, and consensus timing

#### Submission Metrics
- **Submission Overview**: Overall submission timing (started/completed slot offset, latency, success)
- **Submission Endpoints Table**: Per-endpoint results with node_id, submitter_type (mempool/angstrom/mev_boost), endpoint URL, success status, and latency

**Discrepancy Analysis:**
The Submission Endpoints Table shows a union of all endpoints across all nodes, making it easy to:
- Compare which endpoints each node submitted to
- Identify success/failure differences between nodes for the same endpoint
- Analyze latency variations across nodes

## Metrics Reference

All metrics are indexed by `block_number` label for per-block analysis.

### Block Metrics (`ang_block_*`)

| Metric | Labels | Description |
|--------|--------|-------------|
| `ang_block_preproposal_limit_orders` | block_number, node_id | Limit orders when PreProposal created |
| `ang_block_preproposal_searcher_orders` | block_number, node_id | Searcher orders when PreProposal created |
| `ang_block_state_slot_offset_ms` | block_number, state, node_id | Slot offset (ms) when state entered |
| `ang_block_state_limit_orders` | block_number, state, node_id | Limit orders at state |
| `ang_block_state_searcher_orders` | block_number, state, node_id | Searcher orders at state |
| `ang_block_preproposals_collected` | block_number, node_id | PreProposals collected from validators |
| `ang_block_is_leader` | block_number, node_id | 1 if leader, 0 otherwise |
| `ang_block_matching_input_limit_pre_quorum` | block_number, node_id | Limit orders from preproposals (before filter) |
| `ang_block_matching_input_searcher_pre_quorum` | block_number, node_id | Searcher orders from preproposals (before filter) |
| `ang_block_matching_input_limit_post_quorum` | block_number, node_id | Limit orders after quorum filtering |
| `ang_block_matching_input_searcher_post_quorum` | block_number, node_id | Searcher orders after quorum filtering |
| `ang_block_matching_pools_solved` | block_number, node_id | Pools with solutions |
| `ang_block_matching_orders_filled` | block_number, node_id | Completely filled orders |
| `ang_block_matching_orders_partial` | block_number, node_id | Partially filled orders |
| `ang_block_matching_orders_unfilled` | block_number, node_id | Unfilled orders |
| `ang_block_matching_orders_killed` | block_number, node_id | Killed orders |
| `ang_block_bundle_generated` | block_number, node_id | 1=bundle generated, 0=attestation only |
| `ang_block_submission_started_slot_offset_ms` | block_number, node_id | Slot offset when submission started |
| `ang_block_submission_completed_slot_offset_ms` | block_number, node_id | Slot offset when submission completed |
| `ang_block_submission_latency_ms` | block_number, node_id | Total submission latency |
| `ang_block_submission_success` | block_number, node_id | 1=success, 0=failed |
| `ang_block_submission_endpoint_success` | block_number, submitter_type, endpoint, node_id | Per-endpoint success |
| `ang_block_submission_endpoint_latency_ms` | block_number, submitter_type, endpoint, node_id | Per-endpoint latency |
| `ang_block_bundle_included` | block_number, node_id | 1=bundle tx included in block, 0=not included |

### Consensus Metrics (`consensus_*`)

| Metric | Labels | Description |
|--------|--------|-------------|
| `consensus_block_height` | node_id | Current block height |
| `consensus_proposal_build_time_per_block` | block_number, node_id | Proposal build time (ms) |
| `consensus_proposal_verification_time_per_block` | block_number, node_id | Proposal verification time (ms) |
| `consensus_completion_time_per_block` | block_number, node_id | Consensus round completion time (ms) |
