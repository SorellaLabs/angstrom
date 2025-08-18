use angstrom_metrics::initialize_prometheus_metrics;

pub async fn init_metrics(metrics_port: u16) {
    let _ = initialize_prometheus_metrics(metrics_port)
        .await
        .inspect_err(|e| eprintln!("failed to start metrics endpoint - {e:?}"));
}
