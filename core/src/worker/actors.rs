use super::events::{IngestEnvelope, TransientLaneMessage};
use super::{EVENT_FEED_MAX_FAILURES, RETRY_BACKOFF_BASE_SECONDS, RETRY_BACKOFF_MAX_SECONDS};
use crate::connector::{SinkError, SinkLane, TelemetrySink};
use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

pub(super) fn merge_json_value(current: &mut Value, incoming: &Value) {
    match (current, incoming) {
        (Value::Object(current_map), Value::Object(incoming_map)) => {
            for (key, incoming_value) in incoming_map {
                if let Some(current_value) = current_map.get_mut(key) {
                    merge_json_value(current_value, incoming_value);
                } else {
                    current_map.insert(key.clone(), incoming_value.clone());
                }
            }
        }
        (current_value, incoming_value) => {
            *current_value = incoming_value.clone();
        }
    }
}

fn update_live_state_cache(
    master_state: &mut Value,
    cached_match_id: &mut Option<String>,
    envelope: &IngestEnvelope,
) -> Value {
    let incoming_match_id = envelope.active_match_id.trim();
    if !incoming_match_id.is_empty() {
        match cached_match_id.as_deref() {
            Some(cached_match_id_value) if cached_match_id_value != incoming_match_id => {
                *master_state = Value::Object(Map::new());
                *cached_match_id = Some(incoming_match_id.to_string());
            }
            Some(_) => {}
            None => {
                *cached_match_id = Some(incoming_match_id.to_string());
            }
        }
    }

    merge_json_value(master_state, &envelope.payload);
    master_state.clone()
}

pub(super) async fn run_live_state_actor(
    mut receiver: mpsc::Receiver<TransientLaneMessage>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    let mut master_state = Value::Object(Map::new());
    let mut cached_match_id = None;
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            maybe_message = receiver.recv() => {
                let Some(message) = maybe_message else {
                    break;
                };

                match message {
                    TransientLaneMessage::Event(envelope) => {
                        process_live_state_envelope(
                            &envelope,
                            &sink,
                            &mut master_state,
                            &mut cached_match_id,
                        )
                        .await;
                    }
                    TransientLaneMessage::Flush { ack } => {
                        if ack.send(()).is_err() {
                            eprintln!("Live-state flush ack receiver dropped before notification.");
                        }
                    }
                    TransientLaneMessage::Snapshot { result } => {
                        let _ = result.send(master_state.clone());
                    }
                    TransientLaneMessage::Reset => {
                        master_state["score"] = json!({ "blue": 0, "orange": 0 });
                        master_state["player_telemetry"] = json!({});
                        master_state["has_winner"] = json!(false);
                        master_state["winner"] = json!("");
                        master_state["is_overtime"] = json!(false);
                    }
                }
            }
        }
    }
}

async fn process_live_state_envelope(
    envelope: &IngestEnvelope,
    sink: &Arc<dyn TelemetrySink + Send + Sync>,
    master_state: &mut Value,
    cached_match_id: &mut Option<String>,
) {
    let payload_to_send = update_live_state_cache(master_state, cached_match_id, envelope);

    if let Err(err) = sink
        .send_event_on_lane(SinkLane::LiveState, &envelope.event_type, &payload_to_send)
        .await
    {
        log_sink_failure(SinkLane::LiveState, envelope, &err);
    }
}

pub(super) async fn run_event_feed_actor(
    mut receiver: mpsc::Receiver<TransientLaneMessage>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            maybe_message = receiver.recv() => {
                let Some(message) = maybe_message else {
                    break;
                };

                match message {
                    TransientLaneMessage::Event(envelope) => {
                        send_with_retry_policy(
                            SinkLane::EventFeed,
                            &envelope,
                            &sink,
                            &shutdown,
                            Some(EVENT_FEED_MAX_FAILURES),
                        ).await;
                    }
                    TransientLaneMessage::Flush { ack } => {
                        if ack.send(()).is_err() {
                            eprintln!("Event-feed flush ack receiver dropped before notification.");
                        }
                    }
                    TransientLaneMessage::Snapshot { .. } | TransientLaneMessage::Reset => {}
                }
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

                send_with_retry_policy(SinkLane::Historical, &envelope, &sink, &shutdown, None)
                    .await;
            }
        }
    }
}

pub(super) async fn send_with_retry_policy(
    lane: SinkLane,
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

        match sink
            .send_event_on_lane(lane, &envelope.event_type, payload)
            .await
        {
            Ok(()) => return,
            Err(SinkError::Terminal { message }) => {
                eprintln!(
                    "Sink terminal error [{lane}] seq={} event={} match_id={} dropped payload: {}",
                    envelope.seq,
                    envelope.event_type,
                    envelope.active_match_id,
                    message,
                    lane = lane.as_str()
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
                        message,
                        lane = lane.as_str()
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
                    message,
                    lane = lane.as_str()
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

pub(super) fn log_sink_failure(lane: SinkLane, envelope: &IngestEnvelope, error: &SinkError) {
    match error {
        SinkError::RateLimited { message } | SinkError::TransientNetwork { message } => {
            eprintln!(
                "Sink warning [{lane}] seq={} event={} match_id={} error={} (backoff TODO).",
                envelope.seq,
                envelope.event_type,
                envelope.active_match_id,
                message,
                lane = lane.as_str()
            );
        }
        SinkError::Terminal { message } => {
            eprintln!(
                "Sink terminal error [{lane}] seq={} event={} match_id={} dropped payload: {}",
                envelope.seq,
                envelope.event_type,
                envelope.active_match_id,
                message,
                lane = lane.as_str()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_json_value, update_live_state_cache};
    use crate::worker::events::{IngestClass, IngestEnvelope};
    use serde_json::json;

    fn live_envelope(seq: u64, match_id: &str, payload: serde_json::Value) -> IngestEnvelope {
        IngestEnvelope {
            seq,
            event_type: "UpdateState".to_string(),
            payload,
            class: IngestClass::LiveState,
            active_match_id: match_id.to_string(),
        }
    }

    #[test]
    fn merge_json_value_preserves_nested_object_fields() {
        let mut current = json!({
            "score": {
                "blue": 1,
                "orange": 0
            },
            "player_telemetry": {
                "player_1": {
                    "boost": 45,
                    "goals": 1
                }
            }
        });
        let incoming = json!({
            "score": {
                "blue": 2
            },
            "player_telemetry": {
                "player_1": {
                    "shots": 3
                },
                "player_2": {
                    "boost": 80
                }
            }
        });

        merge_json_value(&mut current, &incoming);

        assert_eq!(current["score"]["blue"], json!(2));
        assert_eq!(current["score"]["orange"], json!(0));
        assert_eq!(current["player_telemetry"]["player_1"]["boost"], json!(45));
        assert_eq!(current["player_telemetry"]["player_1"]["goals"], json!(1));
        assert_eq!(current["player_telemetry"]["player_1"]["shots"], json!(3));
        assert_eq!(current["player_telemetry"]["player_2"]["boost"], json!(80));
    }

    #[test]
    fn merge_json_value_replaces_scalars_and_arrays() {
        let mut current = json!({
            "time_remaining_seconds": 180,
            "teams": [0, 1],
            "arena": "DFH Stadium"
        });
        let incoming = json!({
            "time_remaining_seconds": 179,
            "teams": [1, 0],
            "arena": "Champions Field"
        });

        merge_json_value(&mut current, &incoming);

        assert_eq!(current["time_remaining_seconds"], json!(179));
        assert_eq!(current["teams"], json!([1, 0]));
        assert_eq!(current["arena"], json!("Champions Field"));
    }

    #[test]
    fn merge_json_value_replaces_non_object_root() {
        let mut current = json!(null);
        let incoming = json!({
            "match_id": "match_1",
            "time_remaining_seconds": 300
        });

        merge_json_value(&mut current, &incoming);

        assert_eq!(
            current,
            json!({
                "match_id": "match_1",
                "time_remaining_seconds": 300
            })
        );
    }

    #[test]
    fn update_live_state_cache_merges_sparse_updates_for_same_match() {
        let mut master_state = json!({});
        let mut cached_match_id = None;

        let first_payload = update_live_state_cache(
            &mut master_state,
            &mut cached_match_id,
            &live_envelope(
                1,
                "match_1",
                json!({
                    "match_id": "match_1",
                    "time_remaining_seconds": 180,
                    "arena": "DFH Stadium",
                    "score": {"blue": 1, "orange": 0},
                    "player_telemetry": {
                        "player_1": {"boost": 45}
                    }
                }),
            ),
        );

        assert_eq!(first_payload["arena"], json!("DFH Stadium"));
        assert_eq!(
            first_payload["player_telemetry"]["player_1"]["boost"],
            json!(45)
        );

        let second_payload = update_live_state_cache(
            &mut master_state,
            &mut cached_match_id,
            &live_envelope(
                2,
                "match_1",
                json!({
                    "match_id": "match_1",
                    "time_remaining_seconds": 179,
                    "player_telemetry": {
                        "player_1": {"shots": 3}
                    }
                }),
            ),
        );

        assert_eq!(second_payload["arena"], json!("DFH Stadium"));
        assert_eq!(second_payload["time_remaining_seconds"], json!(179));
        assert_eq!(second_payload["score"]["blue"], json!(1));
        assert_eq!(second_payload["score"]["orange"], json!(0));
        assert_eq!(
            second_payload["player_telemetry"]["player_1"]["boost"],
            json!(45)
        );
        assert_eq!(
            second_payload["player_telemetry"]["player_1"]["shots"],
            json!(3)
        );
    }

    #[test]
    fn update_live_state_cache_resets_when_match_changes() {
        let mut master_state = json!({});
        let mut cached_match_id = None;

        let _ = update_live_state_cache(
            &mut master_state,
            &mut cached_match_id,
            &live_envelope(
                1,
                "match_1",
                json!({
                    "match_id": "match_1",
                    "arena": "DFH Stadium",
                    "player_telemetry": {
                        "player_1": {"boost": 45}
                    }
                }),
            ),
        );

        let second_payload = update_live_state_cache(
            &mut master_state,
            &mut cached_match_id,
            &live_envelope(
                2,
                "match_2",
                json!({
                    "match_id": "match_2",
                    "time_remaining_seconds": 300,
                    "score": {"blue": 0, "orange": 0}
                }),
            ),
        );

        assert_eq!(cached_match_id.as_deref(), Some("match_2"));
        assert_eq!(second_payload["match_id"], json!("match_2"));
        assert_eq!(second_payload["time_remaining_seconds"], json!(300));
        assert_eq!(second_payload["score"]["blue"], json!(0));
        assert_eq!(second_payload["score"]["orange"], json!(0));
        assert!(second_payload.get("arena").is_none());
        assert!(second_payload["player_telemetry"].is_null());
    }
}
