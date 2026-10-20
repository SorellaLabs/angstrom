import unittest

from analyze_delivery import read_events, summarize


class DeliveryTest(unittest.TestCase):
    def test_first_builder_response_ignores_slow_failure_and_mempool(self):
        events = read_events([
            '2026-10-20T20:00:08.000000Z INFO auction_stage="cutoff" target_block=42 slot_offset_ms=9000 limit_count=0 searcher_count=60',
            '2026-10-20T20:00:08.002000Z INFO auction_stage="endpoint_response" target_block=42 submitter="mempool" payload_kind="bundle" rpc_success=true success=true',
            '2026-10-20T20:00:08.010000Z INFO auction_stage="endpoint_response" target_block=42 submitter="angstrom" payload_kind="bundle" rpc_success=true success=true',
            '2026-10-20T20:00:38.000000Z INFO auction_stage="endpoint_response" target_block=42 submitter="mev_boost" payload_kind="bundle" rpc_success=false success=false',
            '2026-10-20T20:00:38.001000Z INFO auction_stage="inclusion" target_block=42 included=true',
        ])
        result = summarize(events)
        row = result["per_round"][0]
        self.assertAlmostEqual(row["cutoff_to_first_builder_success_response_ms"], 10, places=3)
        self.assertAlmostEqual(row["cutoff_to_first_mempool_success_response_ms"], 2, places=3)
        self.assertEqual(row["builder_rpc_error_count"], 1)
        self.assertTrue(row["included"])
        self.assertEqual(result["by_order_count"]["51-100"]["rounds"], 1)

    def test_rpc_rejection_and_attestation_are_not_builder_success(self):
        result = summarize(read_events([
            '2026-10-20T20:00:08.000000Z INFO auction_stage="cutoff" target_block=42 slot_offset_ms=9000 limit_count=0 searcher_count=0',
            '2026-10-20T20:00:08.001000Z INFO auction_stage="endpoint_response" target_block=42 submitter="angstrom" payload_kind="bundle" rpc_success=true success=false',
            '2026-10-20T20:00:08.002000Z INFO auction_stage="endpoint_response" target_block=42 submitter="angstrom" payload_kind="unlock_attestation" rpc_success=true success=true',
        ]))
        self.assertEqual(result["rounds_with_builder_success_response"], 0)
        self.assertEqual(result["per_round"][0]["builder_rejection_count"], 1)
        self.assertNotIn("included", result["per_round"][0])

    def test_mixed_nodes_fail_instead_of_fabricating_a_latency(self):
        event = '2026-10-20T20:00:08.000000Z INFO auction_stage="cutoff" target_block=42 slot_offset_ms=9000 limit_count=0 searcher_count=0'
        with self.assertRaisesRegex(ValueError, "multiple cutoffs"):
            summarize(read_events([event, event]))


if __name__ == "__main__":
    unittest.main()
