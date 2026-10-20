#!/usr/bin/env python3
"""Summarize auction_stage journal events, one file per node, without network access."""
import argparse
from collections import defaultdict
import csv
from datetime import datetime
import json
import math
import re

ANSI = re.compile(r"\x1b\[[0-9;]*m")
FIELDS = re.compile(r'(\w+)=(?:"([^"\n]*)"|(\S+))')
BUILDER_SUBMITTERS = {"angstrom", "mev_boost"}
CHANNELS = (("builder", BUILDER_SUBMITTERS), ("mempool", {"mempool"}))
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


def order_bucket(orders):
    return next(name for name, low, high in ORDER_BUCKETS if low <= orders <= high)


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

    endpoints = row["endpoints"] = {}
    for e in sorted(bundle_events(events, BUILDER_SUBMITTERS | {"mempool"}), key=lambda e: e["time"]):
        if e["auction_stage"] not in ("endpoint_request", "endpoint_response"):
            continue
        outcome = endpoints.setdefault("{}#{}".format(*endpoint(e)), {"origin": e.get("endpoint_origin")})
        slot_offset = round(row["cutoff_slot_offset_ms"] + elapsed(e), 3)
        if e["auction_stage"] == "endpoint_request":
            outcome["request_slot_offset_ms"] = slot_offset
        else:
            outcome.update(response_slot_offset_ms=slot_offset, response_ms=int(e["request_elapsed_us"]) / 1000,
                           rpc_success=e["rpc_success"] == "true", success=e["success"] == "true")

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


def summarize_endpoints(rows):
    totals = defaultdict(lambda: {"requests": 0, "missing_responses": 0, "rpc_errors": 0, "rejections": 0,
                                  "successes": 0, "response_ms": [], "success_response_ms": []})
    for row in rows:
        for key, outcome in row["endpoints"].items():
            total = totals[f"{key} {outcome['origin']}"]
            total["requests"] += "request_slot_offset_ms" in outcome
            if "response_ms" not in outcome:
                total["missing_responses"] += 1
                continue
            total["response_ms"].append(outcome["response_ms"])
            total["rpc_errors"] += not outcome["rpc_success"]
            total["rejections"] += outcome["rpc_success"] and not outcome["success"]
            if outcome["success"]:
                total["successes"] += 1
                total["success_response_ms"].append(outcome["response_ms"])
    return {key: {**total, "response_ms": quantiles(total["response_ms"]),
                  "success_response_ms": quantiles(total["success_response_ms"])}
            for key, total in sorted(totals.items())}


def summarize_winners(rows, winners, origin_builders):
    """Judge delivery only against the integrated builder that built the target block."""
    integrated = set(origin_builders.values())
    won = defaultdict(list)
    unmapped = defaultdict(lambda: {"rounds": 0, "included": 0})
    for row in rows:
        winner = winners.get(row["target_block"])
        if winner is None or not row["builder_request_count"]:
            continue
        if winner not in integrated:
            unmapped[winner]["rounds"] += 1
            unmapped[winner]["included"] += row.get("included", False)
            continue
        sent = [outcome for key, outcome in row["endpoints"].items()
                if key.split("#")[0] in BUILDER_SUBMITTERS and origin_builders.get(outcome["origin"]) == winner]
        requests = [o["request_slot_offset_ms"] for o in sent if "request_slot_offset_ms" in o]
        successes = [o["response_slot_offset_ms"] for o in sent if o.get("success")]
        won[winner].append({
            "orders": row["orders"], "included": row.get("included"),
            "send_slot_offset_ms": min(requests) if requests else None,
            "success_response_slot_offset_ms": min(successes) if successes else None,
            "missing_response": any("response_ms" not in o for o in sent)})

    def summary(samples):
        observed = [s for s in samples if s["included"] is not None]
        included = sum(s["included"] for s in observed)
        return {"rounds_won": len(samples),
                "sent": sum(s["send_slot_offset_ms"] is not None for s in samples),
                "with_success_response": sum(s["success_response_slot_offset_ms"] is not None for s in samples),
                "with_missing_response": sum(s["missing_response"] for s in samples),
                "inclusion_observed": len(observed), "included": included,
                "inclusion_rate": round(included / len(observed), 4) if observed else None,
                "send_slot_offset_ms": quantiles(
                    [s["send_slot_offset_ms"] for s in samples if s["send_slot_offset_ms"] is not None]),
                "success_response_slot_offset_ms": quantiles(
                    [s["success_response_slot_offset_ms"] for s in samples if s["success_response_slot_offset_ms"] is not None])}

    return {"builders": {builder: {**summary(samples), "by_order_count": {
                name: summary([s for s in samples if order_bucket(s["orders"]) == name])
                for name, _, _ in ORDER_BUCKETS}}
                for builder, samples in sorted(won.items())},
            "unmapped_winners": dict(sorted(unmapped.items()))}


def summarize(events):
    by_block = defaultdict(list)
    for event in events:
        by_block[event["target_block"]].append(event)
    rows, excluded, without_cutoff = [], [], 0
    for target_block, block_events in sorted(by_block.items()):
        cutoffs = [e for e in block_events if e["auction_stage"] == "cutoff"]
        if not cutoffs:
            without_cutoff += 1  # left-censored, or this node never reached its cutoff
            continue
        try:
            if len(cutoffs) != 1:
                raise Excluded(f"{len(cutoffs)} cutoffs (reorg, restart or mixed nodes)")
            rows.append(summarize_round(target_block, block_events, cutoffs[0]))
        except Excluded as reason:
            excluded.append({"target_block": target_block, "reason": str(reason)})

    metrics = sorted({key for row in rows for key in row if key.endswith("_ms")})
    by_load = {}
    for name, _, _ in ORDER_BUCKETS:
        group = [row for row in rows if order_bucket(row["orders"]) == name]
        by_load[name] = {"rounds": len(group), **attempt_counts(group),
                         **{key: quantiles([row[key] for row in group if key in row]) for key in BUCKET_LATENCIES}}
    return {"rounds": len(rows), "rounds_without_cutoff": without_cutoff, "excluded": excluded,
            **attempt_counts(rows),
            "inclusion_observed": sum("included" in row for row in rows),
            "included": sum(row.get("included", False) for row in rows),
            "latency_ms": {key: quantiles([row[key] for row in rows if key in row]) for key in metrics},
            "by_order_count": by_load, "by_endpoint": summarize_endpoints(rows),
            "per_round": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("journals", nargs="+", help="journalctl -o cat output, one node per file")
    parser.add_argument("--winners", help="CSV with target_block,builder for each target block")
    parser.add_argument("--endpoint-builder", action="append", default=[], metavar="ORIGIN=BUILDER",
                        help="map a builder endpoint origin to its name in --winners")
    args = parser.parse_args()
    nodes = {}
    for path in args.journals:
        with open(path, errors="replace") as source:
            events = read_events(source)
        if not events:
            parser.error(f"{path}: no auction_stage events: check the node runs an instrumented build, "
                         "the window, and that the collection filter matched ANSI-colored lines")
        nodes[path] = summarize(events)
    report = {"nodes": nodes}
    if args.winners:
        with open(args.winners, newline="") as source:
            winners = {int(r["target_block"]): r["builder"] for r in csv.DictReader(source)}
        origin_builders = dict(pair.split("=", 1) for pair in args.endpoint_builder)
        rows = [row for summary in nodes.values() for row in summary["per_round"]]
        report["by_winning_builder"] = summarize_winners(rows, winners, origin_builders)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
