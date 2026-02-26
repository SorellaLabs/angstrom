# Angstrom Grafana Dashboards

## Setup

### Data Source Configuration

The block analysis dashboard now reads per-block history from Postgres and expects a datasource UID of `angstrom-postgres`.

To configure:
1. Go to Grafana → Connections → Data Sources → Add data source.
2. Select PostgreSQL.
3. Point it at the Angstrom metrics database.
4. Set the UID to `angstrom-postgres` (or update the dashboard JSON to your UID).

### Import Dashboard

1. Go to Grafana → Dashboards → Import.
2. Upload `block-analysis.json` or paste its contents.
3. Select the PostgreSQL datasource if prompted.

## Dashboards

### Block Analysis (`block-analysis.json`)

Analyze consensus and submission metrics for a specific block across all nodes.

Usage:
1. Enter a block number in the "Block Number" textbox.
2. Review tables populated from Postgres.

Sections:
- `State Timing Table`: state-level slot offset and order counts.
- `Matching Results Table`: pre/post quorum matching input and matching outcomes.
- `General Consensus Table`: preproposal, leadership, and consensus timings.
- `Submission Overview`: submission offsets, latency, success, and inclusion.
- `Submission Endpoints Table`: per-endpoint submitter outcomes and latency.

Node identity is rendered as `node_address` (signer address).

## Storage Tables

Per-block data is sourced from:
- `ang_block_metrics`
- `ang_block_state_metrics`
- `ang_block_submission_endpoint_metrics`

## Prometheus Scope

Prometheus remains the live monitoring system for bounded metrics (validation, order pool/storage, process/runtime). Metrics with `block_number` labels are no longer served from `/metrics` and are now stored in Postgres.
