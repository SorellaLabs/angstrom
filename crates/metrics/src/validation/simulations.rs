use prometheus::Histogram;

#[derive(Clone)]
pub struct ValidationSimulationMetrics {
    simulate_bundle:    Histogram,
    fetch_gas_for_user: Histogram
}
