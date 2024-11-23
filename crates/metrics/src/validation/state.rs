use prometheus::Histogram;

#[derive(Clone)]
pub struct StateSimulationMetrics {
    loading_balances:           Histogram,
    loading_approvals:          Histogram,
    applying_state_transitions: Histogram
}
