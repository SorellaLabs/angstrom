//! Local request/response observations. A successful RPC response is not proof
//! of builder inclusion, and request start is not a socket-send timestamp.

use std::{future::Future, time::Instant};

use reqwest::Url;

pub(super) async fn observe_endpoint<T, E>(
    target_block: u64,
    submitter: &str,
    endpoint_index: usize,
    url: &Url,
    payload_kind: &str,
    request: impl Future<Output = Result<T, E>>,
    accepted: impl FnOnce(&T) -> bool
) -> (Result<T, E>, u64) {
    // Origin deliberately omits credentials, path, query and fragment. The
    // index distinguishes endpoints sharing an origin without exposing API
    // keys.
    let endpoint_origin = url.origin().ascii_serialization();
    let started = Instant::now();
    tracing::info!(
        auction_stage = "endpoint_request",
        target_block,
        submitter,
        endpoint_index,
        %endpoint_origin,
        payload_kind,
        "auction endpoint request started"
    );
    let result = request.await;
    let elapsed = started.elapsed();
    let rpc_success = result.is_ok();
    let success = result.as_ref().is_ok_and(accepted);
    tracing::info!(
        auction_stage = "endpoint_response",
        target_block,
        submitter,
        endpoint_index,
        %endpoint_origin,
        payload_kind,
        rpc_success,
        success,
        request_elapsed_us = elapsed.as_micros() as u64,
        "auction endpoint response"
    );
    (result, elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        sync::{Arc, Mutex},
        task::Context
    };

    use futures::{
        FutureExt,
        stream::{FuturesUnordered, StreamExt}
    };

    use super::*;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Capture {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    #[tokio::test]
    async fn success_is_observed_before_a_slow_failed_endpoint_finishes() {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let url =
            Url::parse("https://user:password@builder.example/rpc/secret?token=secret").unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let requests = FuturesUnordered::new();
        requests.push(
            observe_endpoint(
                42,
                "angstrom",
                0,
                &url,
                "bundle",
                async { Ok::<_, ()>(true) },
                |ok| *ok
            )
            .boxed()
        );
        requests.push(
            observe_endpoint(
                42,
                "angstrom",
                1,
                &url,
                "bundle",
                async {
                    let _ = rx.await;
                    Err::<bool, _>(())
                },
                |ok| *ok
            )
            .boxed()
        );
        let mut all = requests.collect::<Vec<_>>().boxed();
        let dispatch = tracing::Dispatch::new(subscriber);
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());

        assert!(
            tracing::dispatcher::with_default(&dispatch, || all.poll_unpin(&mut cx)).is_pending()
        );
        let before = capture.text();
        let responses: Vec<_> = before
            .lines()
            .filter(|line| line.contains("auction endpoint response"))
            .collect();
        assert_eq!(responses.len(), 1);
        assert!(responses[0].contains("endpoint_index=0"));
        assert!(responses[0].contains("success=true"));
        assert!(responses[0].contains("https://builder.example"));
        assert!(!before.contains("password"));
        assert!(!before.contains("secret"));

        tx.send(()).unwrap();
        assert!(
            tracing::dispatcher::with_default(&dispatch, || all.poll_unpin(&mut cx)).is_ready()
        );
        let after = capture.text();
        let failed = after
            .lines()
            .find(|line| {
                line.contains("auction endpoint response") && line.contains("endpoint_index=1")
            })
            .unwrap();
        assert!(failed.contains("rpc_success=false"));
        assert!(failed.contains("success=false"));
    }

    #[tokio::test]
    async fn a_rejected_response_is_not_a_success() {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let url = Url::parse("https://builder.example").unwrap();
        let mut response = Box::pin(observe_endpoint(
            42,
            "angstrom",
            0,
            &url,
            "bundle",
            async {
                Ok::<_, ()>(super::super::angstrom::AngstromSubmissionResponse::from_value(
                    serde_json::json!({"success": false, "message": "rejected"})
                ))
            },
            super::super::angstrom::AngstromSubmissionResponse::is_success
        ));
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(
            tracing::subscriber::with_default(subscriber, || response.poll_unpin(&mut cx))
                .is_ready()
        );
        let text = capture.text();
        assert!(text.contains("rpc_success=true success=false"));
    }
}
