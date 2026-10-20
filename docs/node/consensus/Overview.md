# Auction cutoff and submission timing

The local auction cutoff is committed when a round starts. It retains the existing
previous-round timing feedback, configured bounds, random 3–40 ms jitter, and slot
elapsed-time subtraction. Incoming orders do not move that cutoff. The previous
rule subtracted 10 ms per active stored order across all pools, so a new bid could
bring the cutoff forward immediately, including for unrelated pools.

This change does not alter the timing feedback formula or its sigmoid clamp, the
winner selection rule, preproposal aggregation, or builder submission behavior.
In particular, configured `max_wait_duration_ms = 11200` is not a new fixed cutoff.
The existing feedback sample ends when matching and bundle gas estimation return,
**before final encoding, signing and submission**. It is not an end-to-end delivery
measurement. Processing and network reserve still need validation under load.

## Measurement boundaries

Each `auction_stage` event includes `target_block`. Collect one node at a time;
join by that field, not the nearest log timestamp. The stages are:

| Stage | Meaning |
| --- | --- |
| `cutoff` | Local order snapshot obtained for the node's preproposal, including slot offset and order counts. |
| `matching_complete` | Matching and bundle gas estimation returned, with success status. |
| `encoding_complete` | Final bundle encoding returned; payload kind distinguishes a bundle from an empty-block attestation. |
| `submission_started` | Submission task begins before nonce, fee and chain-ID RPC queries. |
| `payload_ready` | A submitter's gas estimation, transaction construction and signing have completed. |
| `endpoint_request` | The endpoint request future is about to be polled. This is not a socket-send timestamp. |
| `endpoint_response` | That endpoint's RPC future returned and its response was interpreted. This event is emitted immediately, without waiting for slower peers. |
| `inclusion` | The target block was fetched and checked for one of the successful submission transaction hashes. |

For a response, `rpc_success` means the RPC operation returned without an error;
`success` additionally applies the submitter's existing application-response
rules. Transport failures and timeouts therefore have both fields false; an
Angstrom rejection or unrecognized result has `success=false`. An HTTP 200 alone
is not success. A successful builder RPC response is **not proof of inclusion**
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
  --no-pager -o cat --grep 'auction_stage=' > auction-delivery.log
```

Copy the log to the analysis machine, preserving one file per node. Run:

```sh
python3 docs/node/consensus/analyze_delivery.py auction-delivery.log > delivery.json
python3 -B -m unittest discover -s docs/node/consensus -p 'test_delivery.py'
```

The report includes per-round data, p50/p95/p99/max latency, counts of missing
successful builder responses, RPC failures versus application rejections, observed
inclusion, and first successful builder response latency by order-count bucket.
Missing inclusion is unknown, not failure. Round counts include non-leaders and
empty rounds; use the per-round request and encoding events to choose the eligible
bundle-submission denominator. Do not compare a raw all-round success rate.
Historical logs without these events cannot reconstruct first successful response
timing; the tool deliberately refuses to infer it from the all-responses event.

## Acceptance before deployment

Compare an instrumented baseline retaining the old cutoff with this candidate in
a representative non-production validation environment. Keep traffic, pool mix,
node topology, endpoint configuration and timing settings comparable. Exercise
normal and high retained-bid counts, limit-order load, slow/rejecting/timing-out
endpoints, and rounds where the Ethereum head arrives late.

Confirm that new bids no longer advance any current round's deadline, prior-round
feedback affects only later rounds, and quorum progress remains healthy. Measure
the full snapshot-to-first-successful-builder-response tail and its slot offset;
include failed or missing responses in the assessment rather than conditioning
only on success. Correlate target-block inclusion and builder rejection reasons.
The candidate must preserve adequate measured headroom for the actual submission
route at high load; no numerical deployment threshold is established by the
historical matching-only sample. A later explicit cutoff or changed feedback
estimator would require a separate decision and validation.

This removes bid-induced cutoff movement. It does not remove deadline sniping or
establish an economic improvement: compare LP reward, inclusion, and bidder net
value separately. No production deployment is authorized by this document.
