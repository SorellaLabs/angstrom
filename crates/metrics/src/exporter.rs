//! Prometheus exporter
use std::{
    convert::Infallible,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc
};

use eyre::WrapErr;
use hyper::{
    Response,
    header::{CONTENT_TYPE, HeaderValue}
};
#[allow(unused_imports)]
use metrics::Unit;
use metrics_exporter_prometheus::PrometheusHandle;
use prometheus::{Encoder, TextEncoder};
use reth_node_metrics::recorder::install_prometheus_recorder;

pub(crate) trait Hook: Fn() + Send + Sync {}
impl<T: Fn() + Send + Sync> Hook for T {}

/// Installs Prometheus as the metrics recorder and serves it over HTTP with
/// hooks.
///
/// The hooks are called every time the metrics are requested at the given
/// endpoint, and can be used to record values for pull-style metrics, i.e.
/// metrics that are not automatically updated.
pub(crate) async fn initialize_with_hooks<F: Hook + 'static>(
    listen_addr: SocketAddr,
    hooks: impl IntoIterator<Item = F>
) -> eyre::Result<()> {
    let recorder = install_prometheus_recorder();
    let handle = recorder.handle().clone();

    let hooks: Vec<_> = hooks.into_iter().collect();

    start_endpoint(listen_addr, handle, Arc::new(move || hooks.iter().for_each(|hook| hook())))
        // Start endpoint
        .await
        .wrap_err("Could not start Prometheus endpoint")?;

    Ok(())
}

async fn start_endpoint<F: Hook + 'static>(
    listen_addr: SocketAddr,
    handle: PrometheusHandle,
    hook: Arc<F>
) -> eyre::Result<()> {
    let listener = tokio::net::TcpListener::bind(listen_addr)
        .await
        .wrap_err("Could not bind to address")?;

    tokio::spawn(async move {
        loop {
            let Ok((io, _)) = listener.accept().await else {
                tracing::error!("metrics listener failed to accept");
                continue;
            };

            let hook = hook.clone();
            let h_clone = handle.clone();
            let service = tower::service_fn(move |_| {
                (hook)();
                h_clone.run_upkeep();
                let mut metrics_render = h_clone.render();

                let mut buffer = Vec::new();
                let encoder = TextEncoder::new();
                // Gather the metrics.
                let metric_families = prometheus::gather();
                // Encode them to send.
                encoder.encode(&metric_families, &mut buffer).unwrap();
                metrics_render += &String::from_utf8(buffer.clone()).unwrap();
                let mut response = Response::new(metrics_render);

                response
                    .headers_mut()
                    .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
                async move { Ok::<_, Infallible>(response) }
            });

            tokio::task::spawn(async move {
                let _ = jsonrpsee_server::serve(io, service)
                    .await
                    .inspect_err(|error| tracing::debug!(%error, "failed to serve request"));
            });
        }
    });

    Ok(())
}

/// Installs Prometheus as the metrics recorder and serves it over HTTP with
/// database and process metrics.
pub async fn initialize_prometheus_metrics(port: u16) -> eyre::Result<()> {
    let process = metrics_process::Collector::default();
    // Clone `process` to move it into the hook and use the original `process` for
    // describe below.
    let cloned_process = process.clone();
    let hooks: Vec<Box<dyn Hook<Output = ()>>> = vec![
        Box::new(move || cloned_process.collect()),
        Box::new(collect_memory_stats),
        Box::new(collect_io_stats),
    ];
    initialize_with_hooks(SocketAddr::new(IpAddr::V4(Ipv4Addr::from([0, 0, 0, 0])), port), hooks)
        .await?;

    // We describe the metrics after the recorder is installed, otherwise this
    // information is not registered
    process.describe();
    describe_memory_stats();
    describe_io_stats();

    Ok(())
}

#[cfg(all(feature = "jemalloc", unix))]
fn collect_memory_stats() {
    use metrics::gauge;
    use tikv_jemalloc_ctl::{epoch, stats};
    use tracing::error;

    if epoch::advance()
        .map_err(|error| error!(%error, "Failed to advance jemalloc epoch"))
        .is_err()
    {
        return;
    }

    if let Ok(value) = stats::active::read()
        .map_err(|error| error!(%error, "Failed to read jemalloc.stats.active"))
    {
        gauge!("jemalloc.active", value as f64);
    }

    if let Ok(value) = stats::allocated::read()
        .map_err(|error| error!(%error, "Failed to read jemalloc.stats.allocated"))
    {
        gauge!("jemalloc.allocated", value as f64);
    }

    if let Ok(value) = stats::mapped::read()
        .map_err(|error| error!(%error, "Failed to read jemalloc.stats.mapped"))
    {
        gauge!("jemalloc.mapped", value as f64);
    }

    if let Ok(value) = stats::metadata::read()
        .map_err(|error| error!(%error, "Failed to read jemalloc.stats.metadata"))
    {
        gauge!("jemalloc.metadata", value as f64);
    }

    if let Ok(value) = stats::resident::read()
        .map_err(|error| error!(%error, "Failed to read jemalloc.stats.resident"))
    {
        gauge!("jemalloc.resident", value as f64);
    }

    if let Ok(value) = stats::retained::read()
        .map_err(|error| error!(%error, "Failed to read jemalloc.stats.retained"))
    {
        gauge!("jemalloc.retained", value as f64);
    }
}

#[cfg(all(feature = "jemalloc", unix))]
fn describe_memory_stats() {
    use metrics::describe_gauge;
    describe_gauge!(
        "jemalloc.active",
        Unit::Bytes,
        "Total number of bytes in active pages allocated by the application"
    );
    describe_gauge!(
        "jemalloc.allocated",
        Unit::Bytes,
        "Total number of bytes allocated by the application"
    );
    describe_gauge!(
        "jemalloc.mapped",
        Unit::Bytes,
        "Total number of bytes in active extents mapped by the allocator"
    );
    describe_gauge!(
        "jemalloc.metadata",
        Unit::Bytes,
        "Total number of bytes dedicated to jemalloc metadata"
    );
    describe_gauge!(
        "jemalloc.resident",
        Unit::Bytes,
        "Total number of bytes in physically resident data pages mapped by the allocator"
    );
    describe_gauge!(
        "jemalloc.retained",
        Unit::Bytes,
        "Total number of bytes in virtual memory mappings that were retained rather than being \
         returned to the operating system via e.g. munmap(2)"
    );
}

#[cfg(not(all(feature = "jemalloc", unix)))]
fn collect_memory_stats() {}

#[cfg(not(all(feature = "jemalloc", unix)))]
fn describe_memory_stats() {}

#[cfg(target_os = "linux")]
fn collect_io_stats() {
    use metrics::absolute_counter;
    use tracing::error;

    let Ok(process) = procfs::process::Process::myself()
        .map_err(|error| error!(%error, "Failed to get currently running process"))
    else {
        return;
    };

    let Ok(io) = process.io().map_err(
        |error| error!(%error, "Failed to get IO stats for the currently running process")
    ) else {
        return;
    };

    absolute_counter!("io.rchar", io.rchar);
    absolute_counter!("io.wchar", io.wchar);
    absolute_counter!("io.syscr", io.syscr);
    absolute_counter!("io.syscw", io.syscw);
    absolute_counter!("io.read_bytes", io.read_bytes);
    absolute_counter!("io.write_bytes", io.write_bytes);
    absolute_counter!("io.cancelled_write_bytes", io.cancelled_write_bytes);
}

#[cfg(target_os = "linux")]
fn describe_io_stats() {
    use metrics::describe_counter;

    describe_counter!("io.rchar", "Characters read");
    describe_counter!("io.wchar", "Characters written");
    describe_counter!("io.syscr", "Read syscalls");
    describe_counter!("io.syscw", "Write syscalls");
    describe_counter!("io.read_bytes", Unit::Bytes, "Bytes read");
    describe_counter!("io.write_bytes", Unit::Bytes, "Bytes written");
    describe_counter!("io.cancelled_write_bytes", Unit::Bytes, "Cancelled write bytes");
}

#[cfg(not(target_os = "linux"))]
fn collect_io_stats() {}

#[cfg(not(target_os = "linux"))]
fn describe_io_stats() {}
