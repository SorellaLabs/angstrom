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
BUILDERS = {"angstrom", "mev_boost"}


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


def summarize(events):
    by_block = defaultdict(list)
    for event in events:
        by_block[event["target_block"]].append(event)
    rows = []
    for target_block, block_events in sorted(by_block.items()):
        cutoffs = [e for e in block_events if e["auction_stage"] == "cutoff"]
        if not cutoffs:
            continue  # left-censored events from an earlier round
        if len(cutoffs) != 1:
            raise ValueError(f"multiple cutoffs for {target_block}; do not mix nodes or restarts")
        cutoff = cutoffs[0]
        row = {"target_block": target_block,
               "orders": int(cutoff["limit_count"]) + int(cutoff["searcher_count"]),
               "cutoff_slot_offset_ms": int(cutoff["slot_offset_ms"])}
        def elapsed(event):
            value = 1000 * (event["time"] - cutoff["time"])
            if value < 0:
                raise ValueError(f"event precedes cutoff for {target_block}; inspect clock/order")
            return value
        for stage in ("matching_complete", "encoding_complete", "submission_started"):
            candidates = [e for e in block_events if e["auction_stage"] == stage]
            if candidates:
                row[f"cutoff_to_{stage}_ms"] = elapsed(min(candidates, key=lambda e: e["time"]))
        for channel, submitters in (("builder", BUILDERS), ("mempool", {"mempool"})):
            channel_events = [e for e in block_events if e.get("submitter") in submitters
                              and e.get("payload_kind") == "bundle"]
            for stage in ("payload_ready", "endpoint_request"):
                candidates = [e for e in channel_events if e["auction_stage"] == stage]
                if candidates:
                    row[f"cutoff_to_first_{channel}_{stage}_ms"] = elapsed(min(candidates, key=lambda e: e["time"]))
            responses = [e for e in channel_events if e["auction_stage"] == "endpoint_response"]
            row[f"{channel}_response_count"] = len(responses)
            row[f"{channel}_rpc_error_count"] = sum(e["rpc_success"] == "false" for e in responses)
            row[f"{channel}_rejection_count"] = sum(e["rpc_success"] == "true" and e["success"] == "false" for e in responses)
            successes = [e for e in responses if e["success"] == "true"]
            if successes:
                first = min(successes, key=lambda e: e["time"])
                latency = elapsed(first)
                row[f"cutoff_to_first_{channel}_success_response_ms"] = latency
                row[f"first_{channel}_success_slot_offset_ms"] = row["cutoff_slot_offset_ms"] + latency
        inclusions = [e for e in block_events if e["auction_stage"] == "inclusion"]
        if inclusions:
            values = {e["included"] for e in inclusions}
            if len(values) != 1:
                raise ValueError(f"inconsistent inclusion outcomes for {target_block}")
            row["included"] = inclusions[0]["included"] == "true"
        rows.append(row)
    metrics = sorted({key for row in rows for key in row if key.endswith("_ms")})
    latency = {key: quantiles([row[key] for row in rows if key in row]) for key in metrics}
    by_load = {}
    for name, low, high in (("0", 0, 0), ("1-25", 1, 25), ("26-50", 26, 50), ("51-100", 51, 100), ("101+", 101, math.inf)):
        group = [row for row in rows if low <= row["orders"] <= high]
        key = "cutoff_to_first_builder_success_response_ms"
        by_load[name] = {"rounds": len(group), "first_builder_success_response_ms": quantiles([row[key] for row in group if key in row])}
    builder_attempts = [row for row in rows if "cutoff_to_first_builder_endpoint_request_ms" in row]
    return {"rounds": len(rows), "rounds_with_builder_request": len(builder_attempts),
            "builder_attempts_without_observed_success_response": sum("cutoff_to_first_builder_success_response_ms" not in row for row in builder_attempts),
            "rounds_with_builder_success_response": sum("cutoff_to_first_builder_success_response_ms" in row for row in rows),
            "inclusion_observed": sum("included" in row for row in rows),
            "included": sum(row.get("included", False) for row in rows),
            "latency_ms": latency, "by_order_count": by_load, "per_round": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("journal", help="journalctl -o cat output, one node per file")
    args = parser.parse_args()
    with open(args.journal) as source:
        events = read_events(source)
    if not events:
        parser.error("no auction_stage events; historical uninstrumented logs cannot measure this path")
    print(json.dumps(summarize(events), indent=2))


if __name__ == "__main__":
    main()
