use crate::connector::{SinkError, TelemetrySink};
use crate::models::{CumulativePlayerStats, MatchIndexEntry, PlayerMatchLog};
use futures_util::future::join_all;
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use super::actors::calculate_full_jitter_backoff;
use super::AGGREGATION_MAX_FAILURES;

pub fn sanitize_firebase_key(key: &str) -> String {
    key.replace(['.', '#', '$', '/', '[', ']'], "_")
}

pub fn build_match_index_entry(match_id: &str, state: &Value) -> MatchIndexEntry {
    let score = state.get("score").and_then(Value::as_object);
    let blue_score = score
        .and_then(|s| s.get("blue"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let orange_score = score
        .and_then(|s| s.get("orange"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u64, |dur| dur.as_secs());

    MatchIndexEntry {
        timestamp,
        blue_score,
        orange_score,
        match_id: match_id.to_string(),
    }
}

pub fn build_player_match_logs(match_id: &str, state: &Value) -> Vec<(String, PlayerMatchLog)> {
    let Some(players) = state
        .get("player_telemetry")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u64, |dur| dur.as_secs());

    players
        .iter()
        .map(|(player_id, player_data)| {
            let sanitized_id = sanitize_firebase_key(player_id);
            (
                sanitized_id,
                PlayerMatchLog {
                    timestamp,
                    goals: extract_u64(player_data, "goals"),
                    shots: extract_u64(player_data, "shots"),
                    saves: extract_u64(player_data, "saves"),
                    assists: extract_u64(player_data, "assists"),
                    score: extract_i64(player_data, "score"),
                    touches: extract_u64(player_data, "touches"),
                    demos: extract_u64(player_data, "demos"),
                    match_id: match_id.to_string(),
                },
            )
        })
        .collect()
}

fn extract_u64(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn extract_i64(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn extract_team(player_data: &Value) -> Option<u64> {
    player_data.get("team").and_then(Value::as_u64)
}

fn compute_match_outcome(
    state: &Value,
    index_entry: &MatchIndexEntry,
) -> (Option<u64>, Option<i64>) {
    let winning_team = match index_entry.blue_score.cmp(&index_entry.orange_score) {
        std::cmp::Ordering::Greater => Some(0_u64),
        std::cmp::Ordering::Less => Some(1_u64),
        std::cmp::Ordering::Equal => None,
    };

    let Some(wt) = winning_team else {
        return (None, None);
    };

    let Some(players) = state
        .get("player_telemetry")
        .and_then(Value::as_object)
    else {
        return (Some(wt), None);
    };

    let max_score = players
        .values()
        .filter(|p| extract_team(p) == Some(wt))
        .filter_map(|p| p.get("score").and_then(Value::as_i64))
        .max();

    (Some(wt), max_score)
}

pub async fn upload_aggregation(
    sink: &Arc<dyn TelemetrySink + Send + Sync>,
    match_id: &str,
    state: &Value,
    shutdown: &CancellationToken,
) {
    let index_entry = build_match_index_entry(match_id, state);
    let player_logs = build_player_match_logs(match_id, state);
    let (winning_team, winning_team_max_score) = compute_match_outcome(state, &index_entry);

    let mut upload_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>> = Vec::new();

    let index_value = match serde_json::to_value(&index_entry) {
        Ok(value) => value,
        Err(err) => {
            error!(
                "Aggregation error: failed to serialize match index entry for match_id={match_id}: {err}"
            );
            Value::Null
        }
    };

    if !index_value.is_null() {
        upload_futures.push(Box::pin(upload_with_retry(
            Arc::clone(sink),
            format!("matches_index/{match_id}"),
            index_value,
            shutdown.clone(),
        )));
    }

    for (sanitized_id, log) in &player_logs {
        let log_value = match serde_json::to_value(log) {
            Ok(value) => value,
            Err(err) => {
                error!(
                    "Aggregation error: failed to serialize player match log for match_id={match_id} player={sanitized_id}: {err}"
                );
                continue;
            }
        };

        upload_futures.push(Box::pin(upload_with_retry(
            Arc::clone(sink),
            format!("player_match_logs/{sanitized_id}/{match_id}"),
            log_value,
            shutdown.clone(),
        )));
    }

    if let Some(players) = state
        .get("player_telemetry")
        .and_then(Value::as_object)
    {
        for (player_id, player_data) in players {
            let sanitized_id = sanitize_firebase_key(player_id);
            let player_team = extract_team(player_data);
            let player_score = player_data.get("score").and_then(Value::as_i64);

            let is_mvp = match (winning_team, winning_team_max_score) {
                (Some(wt), Some(max_score)) => {
                    player_team == Some(wt) && player_score == Some(max_score)
                }
                _ => false,
            };

            if let Some((_, log)) = player_logs
                .iter()
                .find(|(sid, _)| sid == &sanitized_id)
            {
                upload_futures.push(Box::pin(update_cumulative_stats(
                    Arc::clone(sink),
                    sanitized_id.clone(),
                    log,
                    player_team,
                    winning_team,
                    is_mvp,
                    shutdown.clone(),
                )));
            }
        }
    }

    join_all(upload_futures).await;
}

#[expect(
    clippy::too_many_lines,
    reason = "R-M-W flow spans GET, modify, and PUT with retries; splitting would add indirection without reducing overall complexity."
)]
async fn update_cumulative_stats(
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    sanitized_id: String,
    log: &PlayerMatchLog,
    player_team: Option<u64>,
    winning_team: Option<u64>,
    is_mvp: bool,
    shutdown: CancellationToken,
) {
    let path = format!("stats_cumulative/{sanitized_id}");
    let mut failures = 0_u32;

    loop {
        if shutdown.is_cancelled() {
            return;
        }

        let cumulative = match sink.get_node(&path).await {
            Ok(Some(value)) => {
                serde_json::from_value::<CumulativePlayerStats>(value).unwrap_or_else(|err| {
                    warn!(
                        "Aggregation warning: failed to parse cumulative stats for player={sanitized_id}: {err}. Falling back to default."
                    );
                    CumulativePlayerStats::default()
                })
            }
            Ok(None) => CumulativePlayerStats::default(),
            Err(SinkError::Terminal { message }) => {
                error!(
                    "Aggregation error: terminal failure getting cumulative stats for player={sanitized_id}: {message}"
                );
                return;
            }
            Err(
                SinkError::RateLimited { .. } | SinkError::TransientNetwork { .. },
            ) => {
                failures = failures.saturating_add(1);
                if failures > AGGREGATION_MAX_FAILURES {
                    warn!(
                        "Aggregation warning: exceeded max failures ({AGGREGATION_MAX_FAILURES}) getting cumulative stats for player={sanitized_id}"
                    );
                    return;
                }

                let delay = calculate_full_jitter_backoff(failures);
                warn!(
                    "Aggregation warning: retrying get for player={sanitized_id} failures={failures} retrying_in_ms={}.",
                    delay.as_millis()
                );
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = sleep(delay) => {}
                }
                continue;
            }
        };

        let mut cumulative = cumulative;
        cumulative.goals = cumulative.goals.saturating_add(log.goals);
        cumulative.assists = cumulative.assists.saturating_add(log.assists);
        cumulative.saves = cumulative.saves.saturating_add(log.saves);
        cumulative.shots = cumulative.shots.saturating_add(log.shots);
        cumulative.score = cumulative.score.saturating_add(log.score);

        if let (Some(pt), Some(wt)) = (player_team, winning_team) {
            if pt == wt {
                cumulative.wins = cumulative.wins.saturating_add(1);
                if is_mvp {
                    cumulative.mvps = cumulative.mvps.saturating_add(1);
                }
            } else {
                cumulative.losses = cumulative.losses.saturating_add(1);
            }
        }

        let data = match serde_json::to_value(&cumulative) {
            Ok(v) => v,
            Err(err) => {
                error!(
                    "Aggregation error: failed to serialize cumulative stats for player={sanitized_id}: {err}"
                );
                return;
            }
        };

        match sink.put_node(&path, &data).await {
            Ok(()) => return,
            Err(SinkError::Terminal { message }) => {
                error!(
                    "Aggregation error: terminal failure putting cumulative stats for player={sanitized_id}: {message}"
                );
                return;
            }
            Err(
                SinkError::RateLimited { .. } | SinkError::TransientNetwork { .. },
            ) => {
                failures = failures.saturating_add(1);
                if failures > AGGREGATION_MAX_FAILURES {
                    warn!(
                        "Aggregation warning: exceeded max failures ({AGGREGATION_MAX_FAILURES}) putting cumulative stats for player={sanitized_id}"
                    );
                    return;
                }

                let delay = calculate_full_jitter_backoff(failures);
                warn!(
                    "Aggregation warning: retrying put for player={sanitized_id} failures={failures} retrying_in_ms={}.",
                    delay.as_millis()
                );
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = sleep(delay) => {}
                }
            }
        }
    }
}

async fn upload_with_retry(
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    path: String,
    data: Value,
    shutdown: CancellationToken,
) {
    let mut failures = 0_u32;
    loop {
        if shutdown.is_cancelled() {
            return;
        }

        match sink.put_node(&path, &data).await {
            Ok(()) => return,
            Err(SinkError::Terminal { message }) => {
                error!(
                    "Aggregation error: terminal failure putting node={path}: {message}"
                );
                return;
            }
            Err(
                SinkError::RateLimited { .. } | SinkError::TransientNetwork { .. },
            ) => {
                failures = failures.saturating_add(1);
                if failures > AGGREGATION_MAX_FAILURES {
                    warn!(
                        "Aggregation warning: exceeded max failures ({AGGREGATION_MAX_FAILURES}) for node={path}"
                    );
                    return;
                }

                let delay = calculate_full_jitter_backoff(failures);
                warn!(
                    "Aggregation warning: retrying node={path} failures={failures} retrying_in_ms={}.",
                    delay.as_millis()
                );
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = sleep(delay) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sanitize_replaces_firebase_illegal_chars() {
        assert_eq!(sanitize_firebase_key("a.b#c$d/e[f]g"), "a_b_c_d_e_f_g");
        assert_eq!(sanitize_firebase_key("Steam|123|0"), "Steam|123|0");
        assert_eq!(sanitize_firebase_key(""), "");
    }

    #[test]
    fn build_match_index_extracts_scores() {
        let state = json!({
            "score": {
                "blue": 3,
                "orange": 2
            },
            "player_telemetry": {}
        });

        let entry = build_match_index_entry("match_1", &state);
        assert_eq!(entry.blue_score, 3);
        assert_eq!(entry.orange_score, 2);
        assert_eq!(entry.match_id, "match_1");
        assert!(entry.timestamp > 0);
    }

    #[test]
    fn build_match_index_defaults_missing_scores() {
        let state = json!({});
        let entry = build_match_index_entry("match_2", &state);
        assert_eq!(entry.blue_score, 0);
        assert_eq!(entry.orange_score, 0);
    }

    #[test]
    fn build_player_match_logs_extracts_all_stats() {
        let state = json!({
            "player_telemetry": {
                "Player1": {
                    "goals": 2,
                    "shots": 5,
                    "saves": 3,
                    "assists": 1,
                    "score": 450,
                    "touches": 20,
                    "demos": 4
                }
            }
        });

        let logs = build_player_match_logs("match_1", &state);
        assert_eq!(logs.len(), 1);
        let (player_id, log) = &logs[0];
        assert_eq!(player_id, "Player1");
        assert_eq!(log.goals, 2);
        assert_eq!(log.shots, 5);
        assert_eq!(log.saves, 3);
        assert_eq!(log.assists, 1);
        assert_eq!(log.score, 450);
        assert_eq!(log.touches, 20);
        assert_eq!(log.demos, 4);
        assert_eq!(log.match_id, "match_1");
    }

    #[test]
    fn build_player_match_logs_sanitizes_player_ids() {
        let state = json!({
            "player_telemetry": {
                "player.name#test": {
                    "goals": 1,
                    "shots": 0,
                    "saves": 0,
                    "assists": 0,
                    "score": 100,
                    "touches": 0,
                    "demos": 0
                }
            }
        });

        let logs = build_player_match_logs("match_1", &state);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, "player_name_test");
    }

    #[test]
    fn build_player_match_logs_handles_missing_fields() {
        let state = json!({
            "player_telemetry": {
                "Player1": {}
            }
        });

        let logs = build_player_match_logs("match_1", &state);
        assert_eq!(logs.len(), 1);
        let (_, log) = &logs[0];
        assert_eq!(log.goals, 0);
        assert_eq!(log.score, 0);
    }

    #[test]
    fn build_player_match_logs_returns_empty_for_no_telemetry() {
        let state = json!({});
        let logs = build_player_match_logs("match_1", &state);
        assert!(logs.is_empty());
    }

    #[test]
    fn cumulative_stats_default_is_all_zeros() {
        let stats = CumulativePlayerStats::default();
        assert_eq!(stats.goals, 0);
        assert_eq!(stats.assists, 0);
        assert_eq!(stats.saves, 0);
        assert_eq!(stats.shots, 0);
        assert_eq!(stats.wins, 0);
        assert_eq!(stats.losses, 0);
        assert_eq!(stats.mvps, 0);
        assert_eq!(stats.score, 0);
    }

    #[test]
    fn cumulative_stats_deserializes_partial_json() {
        let json = json!({
            "goals": 5,
            "saves": 3
        });
        let stats: CumulativePlayerStats =
            serde_json::from_value(json).unwrap_or_default();
        assert_eq!(stats.goals, 5);
        assert_eq!(stats.saves, 3);
        assert_eq!(stats.assists, 0);
        assert_eq!(stats.shots, 0);
        assert_eq!(stats.wins, 0);
        assert_eq!(stats.losses, 0);
        assert_eq!(stats.mvps, 0);
        assert_eq!(stats.score, 0);
    }

    #[test]
    fn compute_match_outcome_blue_wins() {
        let state = json!({
            "player_telemetry": {
                "Player1": {"team": 0, "score": 450},
                "Player2": {"team": 0, "score": 300},
                "Player3": {"team": 1, "score": 600}
            }
        });
        let index = MatchIndexEntry {
            timestamp: 0,
            blue_score: 3,
            orange_score: 2,
            match_id: "match_1".to_string(),
        };
        let (team, max_score) = compute_match_outcome(&state, &index);
        assert_eq!(team, Some(0));
        assert_eq!(max_score, Some(450));
    }

    #[test]
    fn compute_match_outcome_orange_wins() {
        let state = json!({
            "player_telemetry": {
                "Player1": {"team": 0, "score": 200},
                "Player2": {"team": 1, "score": 500},
                "Player3": {"team": 1, "score": 350}
            }
        });
        let index = MatchIndexEntry {
            timestamp: 0,
            blue_score: 1,
            orange_score: 4,
            match_id: "match_1".to_string(),
        };
        let (team, max_score) = compute_match_outcome(&state, &index);
        assert_eq!(team, Some(1));
        assert_eq!(max_score, Some(500));
    }

    #[test]
    fn compute_match_outcome_draw_returns_none() {
        let state = json!({
            "player_telemetry": {
                "Player1": {"team": 0, "score": 300}
            }
        });
        let index = MatchIndexEntry {
            timestamp: 0,
            blue_score: 2,
            orange_score: 2,
            match_id: "match_1".to_string(),
        };
        let (team, max_score) = compute_match_outcome(&state, &index);
        assert_eq!(team, None);
        assert_eq!(max_score, None);
    }

    #[test]
    fn compute_match_outcome_zeros_returns_none() {
        let state = json!({
            "player_telemetry": {}
        });
        let index = MatchIndexEntry {
            timestamp: 0,
            blue_score: 0,
            orange_score: 0,
            match_id: "match_1".to_string(),
        };
        let (team, max_score) = compute_match_outcome(&state, &index);
        assert_eq!(team, None);
        assert_eq!(max_score, None);
    }

    #[test]
    fn extract_team_from_player_data() {
        let player = json!({"team": 0});
        assert_eq!(extract_team(&player), Some(0));
    }

    #[test]
    fn extract_team_missing_returns_none() {
        let player = json!({});
        assert_eq!(extract_team(&player), None);
    }
}
