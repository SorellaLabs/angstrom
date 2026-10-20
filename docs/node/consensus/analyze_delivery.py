#!/usr/bin/env python3
"""Summarize auction_stage journal events from ONE node, without network access."""
import argparse
from collections import defaultdict
from datetime import datetime
import json
import math
import re

ANSI = re.compile(r"\x1b\[[0-9;]*m")
FIELDS = re.compile(r'(\w+)=(?:"([^"\n]*)"|(\S+))')
CHANNELS = (("builder", {"angstrom", "mev_boost"}), ("mempool", {"mempool"}))
ORDER_BUCKETS = (("0", 0, 0), ("1-25", 1, 25), ("26-50", 26, 50), ("51-100", 51, 100),
                 ("101-250", 101, 250), ("251-500", 251, 500), ("501+", 501, math.inf))
BUCKET_LATENCIES = ("cutoff_to_matching_complete_ms", "matching_ms",
                    "cutoff_to_first_builder_success_response_ms", "first_builder_success_slot_offset_ms")


class Excluded(ValueError):
    """A round whose events cannot be attributed to one cutoff."""


def read_events(lines):
    events = []
    for line_number, raw in enumerate(lines, 1):
        line = ANSI.sub("", raw)
        if "auction_stage=" not in line:
            continue
        fields = {key: quoted or bare for key, quoted, bare in FIELDS.findall(line)}
        try:
            fields["time"] = datetime.fromisoformat(line.split()[0].replace("Z", "+00:00")).timestamp()
            fields["target_block"] = int(fields["target_block"])
            fields["auction_stage"]
        except (ValueError, KeyError, IndexError) as error:
            raise ValueError(f"invalid auction event at line {line_number}") from error
        events.append(fields)
    return events


def quantiles(values):
    if not values:
        return {"n": 0}
    values = sorted(values)
    return {"n": len(values), **{
        label: round(values[max(0, math.ceil(q * len(values)) - 1)], 3)
        for label, q in [("p50", .50), ("p95", .95), ("p99", .99), ("max", 1)]
    }}


def bundle_events(events, submitters):
    return [e for e in events if e.get("submitter") in submitters and e.get("payload_kind") == "bundle"]


def endpoint(event):
    return event["submitter"], event["endpoint_index"]


def summarize_round(target_block, events, cutoff):
    if any(e["time"] < cutoff["time"] for e in events):
        raise Excluded("event precedes cutoff (clock step or restart)")
    row = {"target_block": target_block,
           "orders": int(cutoff["limit_count"]) + int(cutoff["searcher_count"]),
           "cutoff_slot_offset_ms": int(cutoff["slot_offset_ms"])}

    def elapsed(event):
        return round(1000 * (event["time"] - cutoff["time"]), 3)

    def first(candidates):
        return elapsed(min(candidates, key=lambda e: e["time"]))

    for stage in ("matching_started", "matching_complete", "encoding_complete", "submission_started"):
        candidates = [e for e in events if e["auction_stage"] == stage]
        if candidates:
            row[f"cutoff_to_{stage}_ms"] = first(candidates)
    if "cutoff_to_matching_started_ms" in row and "cutoff_to_matching_complete_ms" in row:
        row["matching_ms"] = round(row["cutoff_to_matching_complete_ms"] - row["cutoff_to_matching_started_ms"], 3)

    for channel, submitters in CHANNELS:
        channel_events = bundle_events(events, submitters)
        for stage in ("payload_ready", "endpoint_request"):
            candidates = [e for e in channel_events if e["auction_stage"] == stage]
            if candidates:
                row[f"cutoff_to_first_{channel}_{stage}_ms"] = first(candidates)
        requested = {endpoint(e) for e in channel_events if e["auction_stage"] == "endpoint_request"}
        responses = [e for e in channel_events if e["auction_stage"] == "endpoint_response"]
        successes = [e for e in responses if e["success"] == "true"]
        row[f"{channel}_request_count"] = len(requested)
        row[f"{channel}_missing_response_count"] = len(requested - {endpoint(e) for e in responses})
        row[f"{channel}_rpc_error_count"] = sum(e["rpc_success"] == "false" for e in responses)
        row[f"{channel}_rejection_count"] = sum(e["rpc_success"] == "true" and e["success"] == "false" for e in responses)
        row[f"{channel}_success_count"] = len(successes)
        if successes:
            latency = first(successes)
            row[f"cutoff_to_first_{channel}_success_response_ms"] = latency
            row[f"first_{channel}_success_slot_offset_ms"] = round(row["cutoff_slot_offset_ms"] + latency, 3)

    inclusions = {e["included"] for e in events if e["auction_stage"] == "inclusion"}
    if len(inclusions) > 1:
        raise Excluded("inconsistent inclusion outcomes")
    if inclusions:
        row["included"] = inclusions.pop() == "true"
    return row


def attempt_counts(rows):
    attempts = [row for row in rows if row["builder_request_count"]]
    return {"builder_attempts": len(attempts),
            "builder_attempts_with_success_response": sum(bool(row["builder_success_count"]) for row in attempts),
            "builder_attempts_without_success_response": sum(not row["builder_success_count"] for row in attempts),
            "builder_attempts_with_missing_response": sum(bool(row["builder_missing_response_count"]) for row in attempts)}


def summarize_endpoints(events):
    by_endpoint = defaultdict(lambda: {"requests": 0, "responses": [], "origin": None})
    for e in bundle_events(events, {"angstrom", "mev_boost", "mempool"}):
        if e["auction_stage"] not in ("endpoint_request", "endpoint_response"):
            continue
        stats = by_endpoint[(e["target_block"], *endpoint(e))]
        stats["origin"] = e.get("endpoint_origin")
        if e["auction_stage"] == "endpoint_request":
            stats["requests"] += 1
        elif e["auction_stage"] == "endpoint_response":
            stats["responses"].append(e)
    totals = defaultdict(lambda: {"requests": 0, "missing_responses": 0, "rpc_errors": 0, "rejections": 0,
                                  "successes": 0, "response_ms": [], "success_response_ms": []})
    for (_, submitter, index), stats in by_endpoint.items():
        total = totals[f"{submitter}#{index} {stats['origin']}"]
        total["requests"] += stats["requests"]
        total["missing_responses"] += max(0, stats["requests"] - len(stats["responses"]))
        for e in stats["responses"]:
            response_ms = int(e["request_elapsed_us"]) / 1000
            total["response_ms"].append(response_ms)
            total["rpc_errors"] += e["rpc_success"] == "false"
            total["rejections"] += e["rpc_success"] == "true" and e["success"] == "false"
            if e["success"] == "true":
                total["successes"] += 1
                total["success_response_ms"].append(response_ms)
    return {key: {**total, "response_ms": quantiles(total["response_ms"]),
                  "success_response_ms": quantiles(total["success_response_ms"])}
            for key, total in sorted(totals.items())}


def summarize(events):
    by_block = defaultdict(list)
    for event in events:
        by_block[event["target_block"]].append(event)
    rows, excluded, without_cutoff, included_events = [], [], 0, []
    for target_block, block_events in sorted(by_block.items()):
        cutoffs = [e for e in block_events if e["auction_stage"] == "cutoff"]
        if not cutoffs:
            without_cutoff += 1  # left-censored, or this node never reached its cutoff
            continue
        try:
            if len(cutoffs) != 1:
                raise Excluded(f"{len(cutoffs)} cutoffs (reorg, restart or mixed nodes)")
            rows.append(summarize_round(target_block, block_events, cutoffs[0]))
            included_events.extend(block_events)
        except Excluded as reason:
            excluded.append({"target_block": target_block, "reason": str(reason)})

    metrics = sorted({key for row in rows for key in row if key.endswith("_ms")})
    by_load = {}
    for name, low, high in ORDER_BUCKETS:
        group = [row for row in rows if low <= row["orders"] <= high]
        by_load[name] = {"rounds": len(group), **attempt_counts(group),
                         **{key: quantiles([row[key] for row in group if key in row]) for key in BUCKET_LATENCIES}}
    return {"rounds": len(rows), "rounds_without_cutoff": without_cutoff, "excluded": excluded,
            **attempt_counts(rows),
            "inclusion_observed": sum("included" in row for row in rows),
            "included": sum(row.get("included", False) for row in rows),
            "latency_ms": {key: quantiles([row[key] for row in rows if key in row]) for key in metrics},
            "by_order_count": by_load, "by_endpoint": summarize_endpoints(included_events),
            "per_round": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("journal", help="journalctl -o cat output, one node per file")
    args = parser.parse_args()
    with open(args.journal, errors="replace") as source:
        events = read_events(source)
    if not events:
        parser.error("no auction_stage events: check the node runs an instrumented build, "
                     "the window, and that the collection filter matched ANSI-colored lines")
    print(json.dumps(summarize(events), indent=2))


if __name__ == "__main__":
    main()
