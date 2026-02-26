DELETE FROM ang_block_state_metrics
WHERE updated_at < NOW() - make_interval(days => $1)
