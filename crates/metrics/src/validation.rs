use std::{future::Future, pin::Pin, sync::OnceLock, time::Instant};

use prometheus::{Histogram, HistogramVec, IntGauge};

use crate::METRICS_ENABLED;

#[derive(Clone)]
struct ValidationMetricsInner {
    // generic
    pending_verification:       IntGauge,
    verification_wait_time:     Histogram,
    eth_transition_updates:     Histogram,
    /// doesn't include the time waiting in the pending verification queue
    processing_time:            HistogramVec,
    v4_sim:                     Histogram,
    // simulation
    simulate_bundle:            Histogram,
    fetch_gas_for_user:         HistogramVec,
    // state
    loading_balances:           Histogram,
    loading_angstrom_balances:  Histogram,
    loading_approvals:          Histogram,
    applying_state_transitions: Histogram
}

impl Default for ValidationMetricsInner {
    fn default() -> Self {
        let buckets = prometheus::exponential_buckets(1.0, 2.0, 15).unwrap();

        let pending_verification = prometheus::register_int_gauge!(
            "ang_pending_order_verification",
            "the amount of orders, currently in queue to be verified"
        )
        .unwrap();

        let verification_wait_time = prometheus::register_histogram!(
            "ang_verification_wait_time",
            "the amount of time a order spent in the verification queue",
            buckets.clone()
        )
        .unwrap();

        let eth_transition_updates = prometheus::register_histogram!(
            "ang_verification_update_time",
            "How long it takes to handle a new block update",
            buckets.clone()
        )
        .unwrap();

        let processing_time = prometheus::register_histogram_vec!(
            "ang_verification_processing_time",
            "the total processing time of a order based on it's type",
            &["order_type"],
            buckets.clone()
        )
        .unwrap();

        let simulate_bundle = prometheus::register_histogram!(
            "ang_simulate_bundles_time",
            "how long it takes to simulate a bundle",
            buckets.clone()
        )
        .unwrap();

        let v4_sim = prometheus::register_histogram!(
            "ang_v4_sim_validation",
            "how long we take to simulate a v4 swap",
            buckets.clone()
        )
        .unwrap();

        let fetch_gas_for_user = prometheus::register_histogram_vec!(
            "ang_fetch_user_gas_speed",
            "time to calculate how much gas a user needs to pay",
            &["order_type"],
            buckets.clone()
        )
        .unwrap();

        let loading_balances = prometheus::register_histogram!(
            "ang_loading_balance_time",
            "time to load balanace from db",
            buckets.clone()
        )
        .unwrap();

        let loading_angstrom_balances = prometheus::register_histogram!(
            "ang_loading_angstrom_balances",
            "time to load angstrom balanace from db",
            buckets.clone()
        )
        .unwrap();

        let loading_approvals = prometheus::register_histogram!(
            "ang_loading_approval_time",
            "time to load approvals from db",
            buckets.clone()
        )
        .unwrap();

        let applying_state_transitions = prometheus::register_histogram!(
            "ang_applying_state_transitions_time",
            "how long does it take to apply the new balances and check for expired orders.",
            buckets
        )
        .unwrap();

        Self {
            v4_sim,
            loading_angstrom_balances,
            pending_verification,
            verification_wait_time,
            eth_transition_updates,
            processing_time,
            simulate_bundle,
            fetch_gas_for_user,
            loading_balances,
            loading_approvals,
            applying_state_transitions
        }
    }
}
macro_rules! default_time_metric {
    ($($name:ident),*) => (
        $(
            fn $name<T>(&self, f: impl FnOnce() ->T) -> T {
                let start = Instant::now();
                let r = f();
                let elapsed = start.elapsed().as_micros() as f64;
                self.$name.observe(elapsed);

                r
            }
        )*
    )
}

impl ValidationMetricsInner {
    default_time_metric!(
        eth_transition_updates,
        simulate_bundle,
        loading_approvals,
        loading_balances,
        loading_angstrom_balances
    );

    async fn applying_state_transitions<T>(&self, f: impl AsyncFnOnce() -> T) -> T {
        let start = Instant::now();
        let r = f().await;
        let elapsed = start.elapsed().as_micros() as f64;
        self.applying_state_transitions.observe(elapsed);

        r
    }

    async fn v4_sim<T>(&self, f: impl AsyncFnOnce() -> T) -> T {
        let start = Instant::now();
        let r = f().await;
        let elapsed = start.elapsed().as_micros() as f64;
        self.v4_sim.observe(elapsed);

        r
    }

    fn inc_pending(&self) {
        self.pending_verification.inc();
    }

    fn dec_pending(&self) {
        self.pending_verification.dec();
    }

    async fn handle_pending<'a, T>(
        &self,
        f: impl FnOnce() -> Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>
    ) -> T {
        self.inc_pending();
        let start = Instant::now();
        let r = f().await;
        let elapsed = start.elapsed().as_micros() as f64;
        self.verification_wait_time.observe(elapsed);
        self.dec_pending();

        r
    }

    fn fetch_gas_for_user<T>(&self, is_searcher: bool, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let r = f();
        let elapsed = start.elapsed().as_nanos() as f64;
        self.fetch_gas_for_user
            .with_label_values(&[if is_searcher { "searcher" } else { "limit" }])
            .observe(elapsed);

        r
    }

    async fn new_order<T, F>(&self, is_searcher: bool, f: T)
    where
        T: FnOnce() -> F,
        F: Future<Output = ()>
    {
        let start = Instant::now();
        f().await;
        let elapsed = start.elapsed().as_micros() as f64;
        self.processing_time
            .with_label_values(&[if is_searcher { "searcher" } else { "limit" }])
            .observe(elapsed);
    }
}

static METRICS_INSTANCE: OnceLock<ValidationMetrics> = OnceLock::new();

#[derive(Clone)]
pub struct ValidationMetrics(Option<ValidationMetricsInner>);

macro_rules! delegate_metric {
    ($($name:ident),*) => {
        $(
            pub fn $name<T> (&self, f: impl FnOnce()->T ) -> T {
                if let Some(inner) = self.0.as_ref() {
                    let res = inner.$name(f);

                    return res
                }

                f()
            }
        )*
    };
}

impl Default for ValidationMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationMetrics {
    delegate_metric!(
        eth_transition_updates,
        simulate_bundle,
        loading_approvals,
        loading_balances,
        loading_angstrom_balances
    );

    pub fn new() -> Self {
        METRICS_INSTANCE
            .get_or_init(|| {
                Self(
                    METRICS_ENABLED
                        .get()
                        .copied()
                        .unwrap_or_default()
                        .then(ValidationMetricsInner::default)
                )
            })
            .clone()
    }

    pub async fn applying_state_transitions<T>(&self, f: impl AsyncFnOnce() -> T) -> T {
        if let Some(inner) = self.0.as_ref() {
            return inner.applying_state_transitions(f).await;
        }

        f().await
    }

    pub async fn measure_wait_time<'a, T>(
        &self,
        f: impl FnOnce() -> Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>
    ) -> T {
        if let Some(inner) = self.0.as_ref() {
            return inner.handle_pending(f).await;
        }

        f().await
    }

    pub async fn v4_sim<T>(&self, f: impl AsyncFnOnce() -> T) -> T {
        if let Some(inner) = self.0.as_ref() {
            return inner.v4_sim(f).await;
        }

        f().await
    }

    pub async fn new_order<T, F>(&self, is_searcher: bool, f: T)
    where
        T: FnOnce() -> F,
        F: Future<Output = ()>
    {
        if let Some(inner) = self.0.as_ref() {
            inner.new_order(is_searcher, f).await;

            return;
        }

        f().await;
    }

    pub fn fetch_gas_for_user<T>(&self, is_searcher: bool, f: impl FnOnce() -> T) -> T {
        if let Some(inner) = self.0.as_ref() {
            return inner.fetch_gas_for_user(is_searcher, f);
        }

        f()
    }
}
