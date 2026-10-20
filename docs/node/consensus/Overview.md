# Auction cutoff and submission timing

The local auction cutoff is committed when a round starts: the node's wait
template plus 3–40 ms jitter, minus the time already elapsed in the slot. Incoming
orders do not move it. The previous rule subtracted 10 ms per active stored order
across all pools, so a new bid could bring the cutoff forward immediately,
including for unrelated pools.

Prior-round feedback changes the template only for later rounds, and only after a
round this node led. Its sample runs from round start until matching and bundle
gas estimation return, **before final encoding, signing and submission**; it is
not an end-to-end delivery measurement. Every feedback step lengthens the wait;
the round's duration sets only the step size. A leader round that never reaches
proposal shortens the template by 200 ms.

The sigmoid that maps the template into the configured range is centred at
`min + max/2`, above the range. After a normal leader round its output is close
to `min_wait_duration_ms`, so the cutoff is in practice `min_wait_duration_ms`
plus jitter; before a node first leads, it is the configured midpoint. A leader
round whose sample exceeds `max_wait_duration_ms` by a few hundred milliseconds
moves that node's next cutoff later; one about 1 s longer moves it to about
`max_wait_duration_ms` (11.2 s with `max_wait_duration_ms = 11200`), where later
slow rounds keep it. Re-centring the sigmoid would move normal cutoffs toward
`max_wait_duration_ms` and needs a separate decision. Winner selection,
preproposal aggregation and builder submission are unchanged. Processing and
network reserve still need validation under load.

## Measurement boundaries

Each `auction_stage` event includes `target_block`. Collect one node at a time;
join by that field, not the nearest log timestamp. The stages are:

| Stage | Meaning |
| --- | --- |
| `cutoff` | Local order snapshot obtained for the node's preproposal, including slot offset and order counts. |
| `matching_started` | The leader holds 2/3 preproposal aggregations and starts building matching input. Cutoff to here is quorum wait. |
| `matching_complete` | Matching and bundle gas estimation returned, with success status. |
| `encoding_complete` | Final bundle encoding returned; payload kind distinguishes a bundle from an empty-block attestation. |
| `submission_started` | Submission task begins before nonce, fee and chain-ID RPC queries. |
| `payload_ready` | A submitter's gas estimation, transaction construction and signing have completed. |
| `endpoint_request` | The endpoint request future is about to be polled. This is not a socket-send timestamp. |
| `endpoint_response` | That endpoint's RPC future returned and its response was interpreted. This event is emitted immediately, without waiting for slower peers. |
| `inclusion` | The target block was fetched and checked for one of the successful submission transaction hashes. |

For a response, `rpc_success` means the RPC operation returned without an error;
`success` additionally applies the submitter's existing application-response
rules. Transport failures have both fields false; an Angstrom rejection or
unrecognized result has `success=false`. An HTTP 200 alone is not success. The
submission clients set no request timeout, so a stalled endpoint leaves an
`endpoint_request` without an `endpoint_response`, and the round has no
`inclusion` event because inclusion is checked only after every submitter
returns. A successful builder RPC response is **not proof of inclusion**
or proof of the builder's internal acceptance time. Mempool success is tracked
separately from builder success. Empty-block attestations are not bundle delivery.

Endpoint labels use URL origin and the stable index in that submitter's configured
endpoint list. They omit credentials, path, query and fragment. Keep endpoint
configuration unchanged when comparing index-specific results.

`request_elapsed_us` uses a monotonic clock. Cross-stage elapsed times in the
analysis use log timestamps on the same host. Exclude intervals with clock steps,
restarts, missing logs or mixed node streams; a successful response without a
recorded snapshot cannot establish a cutoff-to-response latency.

The older `submitted bundle` event waited for every submitter and endpoint to
finish. Its replacement is explicitly named `all bundle submission responses
collected`. Neither event measures the first useful response.

## Read-only collection and analysis

On each validator, collect a sufficiently long window including delayed responses
and inclusion results after its final sampled cutoff:

```sh
journalctl -u angstrom.service --since 'START UTC' --until 'END UTC' \
  --no-pager -o cat --all | grep -a 'auction_stage' > auction-delivery.log
```

The node logs in reth's default terminal format with ANSI colors, which put
escape codes between each field name and `=`. Filter on `auction_stage` alone; the
analyzer strips the codes. JSON and logfmt log formats are not supported.

Copy the logs to the analysis machine, one file per node. Run:

```sh
python3 docs/node/consensus/analyze_delivery.py node-*.log > delivery.json
python3 -B -m unittest discover -s docs/node/consensus -p 'test_delivery.py'
```

The report has one summary per node file. Each includes per-round rows, with
every endpoint's request and response slot offsets, and p50/p95/p99/max
latencies. A builder attempt is a round with a builder bundle request; the
summary counts attempts with and without a successful response and, separately,
attempts with at least one missing response. Per endpoint it gives requests,
missing responses, RPC errors, rejections, successes and response-time
quantiles. Per order-count bucket it gives the same attempt counts with
cutoff-to-matching-complete, matching (`matching_ms`) and first-builder-success
latencies. A missing response means a stalled endpoint or a response after the
collection window. Latency quantiles cover only rounds where the event exists;
read them next to the attempt counts, not as a success rate. Rounds with more
than one cutoff (reorg, restart or mixed nodes) or with events before their
cutoff are excluded and listed with the reason. Missing inclusion is unknown,
not failure.
Historical logs without these events cannot reconstruct first successful response
timing; the tool deliberately refuses to infer it from the all-responses event.

### Inclusion by the builder that built the block

A bundle can only land in a block built by a builder that receives it, so a miss
on a block built by a builder outside our endpoints says nothing about send time.
Export `target_block,builder` for the window from the indexer (for example
`builder_name` in `mainnet.block_builder_metrics`), name each builder endpoint
by its origin (as logged in `endpoint_origin`), and run all node files together:

```sh
python3 docs/node/consensus/analyze_delivery.py node-*.log --winners winners.csv \
  --endpoint-builder https://rpc.buildernet.org=BuilderNet > delivery.json
```

`by_winning_builder.builders` then covers only leader rounds that sent a bundle
and whose target block that builder won. Per builder, overall and per
order-count bucket, it gives rounds won, rounds that sent to that builder's
endpoints and that got a successful or missing response from them, inclusion
counts and rate over rounds with an observed outcome, and the slot offsets of the
first request to that builder (`send_slot_offset_ms`) and of its first
successful response. `unmapped_winners` counts bundle rounds whose block was
built by a builder with no mapped origin. The request offset is when the
request future is first polled, after nonce, fee, gas estimation and signing; an
indexer `submission_started` offset plus endpoint `latency_ms` omits that work
and includes the response path, so it is neither send nor arrival time.

## Acceptance before deployment

Compare an instrumented baseline retaining the old cutoff (`20011a2a`) with this
candidate in a representative non-production validation environment. Keep traffic, pool mix,
node topology, endpoint configuration and timing settings comparable. Exercise
normal and high retained-bid counts, limit-order load, slow/rejecting/timing-out
endpoints, and rounds where the Ethereum head arrives late.

Confirm that new bids no longer advance any current round's deadline, prior-round
feedback affects only later rounds, and quorum progress remains healthy. Force a
leader round whose matching completes more than 1 s after
`max_wait_duration_ms + 200 ms` and record that node's next cutoff. Inject one slow and one non-responding endpoint and read `by_endpoint`. Measure
the full snapshot-to-first-successful-builder-response tail and its slot offset;
include failed or missing responses in the assessment rather than conditioning
only on success. Correlate target-block inclusion and builder rejection reasons.
The candidate must preserve adequate measured headroom for the actual submission
route at high load. Set a numerical deadline for the first successful builder
response slot offset before the comparison; the historical matching-only sample
does not establish one. A later explicit cutoff or changed feedback
estimator would require a separate decision and validation.

Judge send time per integrated builder on the blocks it won
(`by_winning_builder`), by order-count bucket, baseline against candidate.
The old rule moved the cutoff 10 ms earlier per stored order, so the baseline
has no busy rounds sending near the template time. Builder inclusion exists only
on mainnet, so this comparison runs there in steps: the baseline, then this
candidate at `min_wait_duration_ms = 9000`, then 9500 for about a day. Accept a
step if inclusion per integrated builder, on the blocks it won and per
order-count bucket, stays within about one percentage point of the baseline.
Report the round counts with each rate: at about 96 % inclusion, telling a
one-point difference from noise at 95 % confidence takes about 3,000 rounds per
builder and bucket in both the baseline and the step, so a day can judge only the
largest builder and bucket combinations; pool buckets or extend the step for the
rest. Report each builder's send
and first-success response slot offsets (p50, p95, p99, max) with the rates; a
9.5 s minimum moves sends past anything the baseline has observed.

Each step restarts the validators. With three validators a quorum needs two, so
restart one at a time. A restarting node can hit the existing panic in
`crates/types/src/block_sync.rs` ("Consensus tried to sign off on a proposal that
didn't exist"); systemd restarts it, costing about 70 s each time. Exclude the
rounds around every restart from both arms. Measure matching time on the
validators' own build (`maxperf` profile, jemalloc, `target-cpu=native`), not a
debug or CI build.

This removes bid-induced cutoff movement. It does not remove deadline sniping or
establish an economic improvement: compare LP reward, inclusion, and bidder net
value separately. No production deployment is authorized by this document.
