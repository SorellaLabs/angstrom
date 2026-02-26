DELETE FROM ang_block_submission_endpoint_metrics
WHERE observed_at < NOW() - make_interval(days => $1)
