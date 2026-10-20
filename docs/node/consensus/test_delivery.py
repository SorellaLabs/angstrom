import unittest

from analyze_delivery import read_events, summarize, summarize_winners

CUTOFF = '2026-10-20T20:00:08.000000Z INFO auction_stage="cutoff" target_block={block} slot_offset_ms=8000 limit_count=0 searcher_count={orders}'


def endpoint_event(time, stage, submitter, index, block=42, payload="bundle", outcome=""):
    return (f'2026-10-20T20:00:{time}Z INFO auction_stage="{stage}" target_block={block} submitter="{submitter}" '
            f'endpoint_index={index} endpoint_origin=https://{submitter}{index}.example payload_kind="{payload}" {outcome}')


def response(time, submitter, index, rpc_success, success, block=42, payload="bundle", elapsed_us=1000):
    return endpoint_event(time, "endpoint_response", submitter, index, block, payload,
                          f"rpc_success={rpc_success} success={success} request_elapsed_us={elapsed_us}")


class DeliveryTest(unittest.TestCase):
    def test_first_builder_response_ignores_slow_failure_and_mempool(self):
        result = summarize(read_events([
            CUTOFF.format(block=42, orders=60),
            *(f'2026-10-20T20:00:08.000500Z INFO auction_stage="payload_ready" target_block=42 '
              f'submitter="{submitter}" payload_kind="bundle"' for submitter in ("mempool", "angstrom", "mev_boost")),
            endpoint_event("08.001000", "endpoint_request", "mempool", 0),
            endpoint_event("08.001000", "endpoint_request", "angstrom", 0),
            endpoint_event("08.001000", "endpoint_request", "mev_boost", 0),
            response("08.002000", "mempool", 0, "true", "true"),
            response("08.010000", "angstrom", 0, "true", "true"),
            response("38.000000", "mev_boost", 0, "false", "false"),
            '2026-10-20T20:00:38.001000Z INFO auction_stage="inclusion" target_block=42 included=true',
        ]))
        row = result["per_round"][0]
        self.assertAlmostEqual(row["cutoff_to_first_builder_success_response_ms"], 10, places=3)
        self.assertAlmostEqual(row["cutoff_to_first_mempool_success_response_ms"], 2, places=3)
        self.assertEqual(row["builder_rpc_error_count"], 1)
        self.assertTrue(row["included"])
        self.assertEqual(result["by_order_count"]["51-100"]["builder_attempts_with_success_response"], 1)
        self.assertEqual(result["by_endpoint"]["mev_boost#0 https://mev_boost0.example"]["rpc_errors"], 1)

    def test_rpc_rejection_and_attestation_are_not_builder_success(self):
        result = summarize(read_events([
            CUTOFF.format(block=42, orders=0),
            endpoint_event("08.000500", "endpoint_request", "angstrom", 0),
            response("08.001000", "angstrom", 0, "true", "false"),
            response("08.002000", "angstrom", 1, "true", "true", payload="unlock_attestation"),
        ]))
        self.assertEqual(result["builder_attempts_without_success_response"], 1)
        self.assertEqual(result["per_round"][0]["builder_rejection_count"], 1)
        self.assertNotIn("included", result["per_round"][0])

    def test_unanswered_request_is_missing_not_rejected(self):
        result = summarize(read_events([
            CUTOFF.format(block=42, orders=0),
            endpoint_event("08.001000", "endpoint_request", "mev_boost", 0),
            endpoint_event("08.001000", "endpoint_request", "mev_boost", 1),
            response("08.020000", "mev_boost", 0, "true", "true", elapsed_us=19000),
        ]))
        row = result["per_round"][0]
        self.assertEqual((row["builder_request_count"], row["builder_missing_response_count"]), (2, 1))
        self.assertEqual(row["builder_rejection_count"], 0)
        self.assertEqual(result["builder_attempts_with_missing_response"], 1)
        endpoints = result["by_endpoint"]
        self.assertEqual(endpoints["mev_boost#1 https://mev_boost1.example"]["missing_responses"], 1)
        self.assertEqual(endpoints["mev_boost#0 https://mev_boost0.example"]["success_response_ms"]["max"], 19)

    def test_order_buckets_count_failed_attempts(self):
        lines = []
        for block in range(10, 20):
            lines += [CUTOFF.format(block=block, orders=600),
                      endpoint_event("08.001000", "endpoint_request", "mev_boost", 0, block=block),
                      response("08.010000", "mev_boost", 0, "true", "true" if block == 10 else "false", block=block)]
        bucket = summarize(read_events(lines))["by_order_count"]["501+"]
        self.assertEqual((bucket["builder_attempts"], bucket["builder_attempts_without_success_response"]), (10, 9))

    def test_rerun_round_is_excluded_without_discarding_the_report(self):
        result = summarize(read_events([
            CUTOFF.format(block=42, orders=0),
            CUTOFF.format(block=42, orders=0).replace("08.000000", "09.000000"),
            CUTOFF.format(block=43, orders=0),
        ]))
        self.assertEqual(result["rounds"], 1)
        self.assertEqual(result["excluded"][0]["target_block"], 42)

    def test_event_before_cutoff_excludes_the_round(self):
        result = summarize(read_events([
            CUTOFF.format(block=42, orders=0),
            response("07.999000", "mev_boost", 0, "false", "false"),
        ]))
        self.assertEqual((result["rounds"], result["excluded"][0]["target_block"]), (0, 42))

    def test_delivery_is_judged_only_on_blocks_the_mapped_builder_won(self):
        def round_lines(block, success, included):
            return [CUTOFF.format(block=block, orders=60),
                    endpoint_event("08.001000", "endpoint_request", "mev_boost", 0, block=block),
                    endpoint_event("08.001000", "endpoint_request", "angstrom", 1, block=block),
                    response("08.020000", "mev_boost", 0, "true", success, block=block),
                    response("08.030000", "angstrom", 1, "true", "true", block=block),
                    f'2026-10-20T20:00:20.000000Z INFO auction_stage="inclusion" target_block={block} included={included}']
        bombora_only = [CUTOFF.format(block=46, orders=60),
                        endpoint_event("08.001000", "endpoint_request", "angstrom", 1, block=46)]
        rows = summarize(read_events(round_lines(42, "true", "true") + round_lines(43, "true", "false")
                                     + round_lines(44, "false", "false") + [CUTOFF.format(block=45, orders=0)]
                                     + bombora_only))["per_round"]
        report = summarize_winners(rows, {42: "Titan", 43: "Eureka", 44: "Titan", 45: "Titan", 46: "Titan"},
                                   {"https://mev_boost0.example": "Titan", "https://angstrom1.example": "Bombora"})
        titan = report["builders"]["Titan"]
        self.assertEqual((titan["rounds_won"], titan["sent"], titan["with_success_response"]), (3, 2, 1))
        self.assertEqual((titan["inclusion_observed"], titan["included"], titan["inclusion_rate"]), (2, 1, 0.5))
        self.assertEqual(titan["send_slot_offset_ms"]["max"], 8001)
        self.assertEqual(titan["success_response_slot_offset_ms"]["max"], 8020)
        self.assertEqual(titan["by_order_count"]["51-100"]["rounds_won"], 3)
        self.assertEqual(report["unmapped_winners"], {"Eureka": {"rounds": 1, "included": 0}})

    def test_ansi_colored_terminal_lines_parse(self):
        line = ('\x1b[2m2026-10-20T20:00:08.000000Z\x1b[0m \x1b[32m INFO\x1b[0m msg '
                '\x1b[3mauction_stage\x1b[0m\x1b[2m=\x1b[0m"cutoff" \x1b[3mtarget_block\x1b[0m\x1b[2m=\x1b[0m42 '
                '\x1b[3mslot_offset_ms\x1b[0m\x1b[2m=\x1b[0m8000 \x1b[3mlimit_count\x1b[0m\x1b[2m=\x1b[0m1 '
                '\x1b[3msearcher_count\x1b[0m\x1b[2m=\x1b[0m2')
        self.assertEqual(summarize(read_events([line]))["per_round"][0]["orders"], 3)


if __name__ == "__main__":
    unittest.main()
