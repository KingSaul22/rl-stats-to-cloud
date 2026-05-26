use super::events::IngestEnvelope;
use super::{EVENT_FEED_MAX_FAILURES, RETRY_BACKOFF_BASE_SECONDS, RETRY_BACKOFF_MAX_SECONDS};
use crate::connector::{SinkError, TelemetrySink};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

pub(super) async fn run_live_state_actor(
    mut receiver: watch::Receiver<IngestEnvelope>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    let mut last_sent_seq = 0_u64;
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            changed = receiver.changed() => {
                if changed.is_err() {
                    break;
                }

                let envelope = receiver.borrow_and_update().clone();
                if envelope.seq <= last_sent_seq {
                    continue;
                }
                last_sent_seq = envelope.seq;

                if let Err(err) = sink.send_event(&envelope.event_type, &envelope.payload).await {
                    log_sink_failure("live_state", &envelope, &err);
                }
            }
        }
    }
}

pub(super) async fn run_event_feed_actor(
    mut receiver: mpsc::Receiver<IngestEnvelope>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            maybe_envelope = receiver.recv() => {
                let Some(envelope) = maybe_envelope else {
                    break;
                };

                send_with_retry_policy(
                    "event_feed",
                    &envelope,
                    &sink,
                    &shutdown,
                    Some(EVENT_FEED_MAX_FAILURES),
                ).await;
            }
        }
    }
}

pub(super) async fn run_historical_actor(
    mut receiver: mpsc::Receiver<IngestEnvelope>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            maybe_envelope = receiver.recv() => {
                let Some(envelope) = maybe_envelope else {
                    break;
                };

                send_with_retry_policy("historical", &envelope, &sink, &shutdown, None).await;
            }
        }
    }
}

pub(super) async fn send_with_retry_policy(
    lane: &str,
    envelope: &IngestEnvelope,
    sink: &Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: &CancellationToken,
    max_failures: Option<u32>,
) {
    let mut consecutive_failures = 0_u32;

    loop {
        if shutdown.is_cancelled() {
            return;
        }

        let payload: &Value = &envelope.payload;

        match sink.send_event(&envelope.event_type, payload).await {
            Ok(()) => return,
            Err(SinkError::Terminal { message }) => {
                eprintln!(
                    "Sink terminal error [{lane}] seq={} event={} match_id={} dropped payload: {}",
                    envelope.seq, envelope.event_type, envelope.active_match_id, message
                );
                return;
            }
            Err(SinkError::RateLimited { message } | SinkError::TransientNetwork { message }) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                if let Some(limit) = max_failures
                    && consecutive_failures > limit
                {
                    eprintln!(
                        "Sink retry budget exceeded [{lane}] seq={} event={} match_id={} failures={} dropped payload: {}",
                        envelope.seq,
                        envelope.event_type,
                        envelope.active_match_id,
                        consecutive_failures,
                        message
                    );
                    return;
                }

                let backoff_delay = calculate_full_jitter_backoff(consecutive_failures);
                eprintln!(
                    "Sink transient failure [{lane}] seq={} event={} match_id={} failures={} retrying_in_ms={} error={}",
                    envelope.seq,
                    envelope.event_type,
                    envelope.active_match_id,
                    consecutive_failures,
                    backoff_delay.as_millis(),
                    message
                );

                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = sleep(backoff_delay) => {}
                }
            }
        }
    }
}

pub(super) fn calculate_full_jitter_backoff(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(5);
    let max_seconds = RETRY_BACKOFF_BASE_SECONDS
        .saturating_mul(1_u64 << exponent)
        .min(RETRY_BACKOFF_MAX_SECONDS);
    let max_window = Duration::from_secs(max_seconds);

    let max_millis = duration_millis_u64(max_window);
    let jitter_millis = sample_uniform_jitter_millis(max_millis);
    Duration::from_millis(jitter_millis)
}

pub(super) fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).map_or(u64::MAX, |value| value)
}

pub(super) fn sample_uniform_jitter_millis(max_millis_inclusive: u64) -> u64 {
    if max_millis_inclusive == 0 {
        return 0;
    }

    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_nanos());
    let modulus = u128::from(max_millis_inclusive).saturating_add(1);
    let sampled = epoch_nanos % modulus;

    u64::try_from(sampled).map_or(max_millis_inclusive, |value| value)
}

pub(super) fn log_sink_failure(lane: &str, envelope: &IngestEnvelope, error: &SinkError) {
    match error {
        SinkError::RateLimited { message } | SinkError::TransientNetwork { message } => {
            eprintln!(
                "Sink warning [{lane}] seq={} event={} match_id={} error={} (backoff TODO).",
                envelope.seq, envelope.event_type, envelope.active_match_id, message
            );
        }
        SinkError::Terminal { message } => {
            eprintln!(
                "Sink terminal error [{lane}] seq={} event={} match_id={} dropped payload: {}",
                envelope.seq, envelope.event_type, envelope.active_match_id, message
            );
        }
    }
}
