DELETE FROM ang_block_metrics
WHERE last_updated_at < NOW() - make_interval(days => $1)
