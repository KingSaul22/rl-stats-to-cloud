use crate::connector::{SinkError, TelemetrySink};
use crate::models::{
    CumulativePlayerStats, CumulativeTeamStats, MatchIndexEntry, PlayerMatchLog,
    PlayerRegistryEntry,
};
use futures_util::future::{join, join_all};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use super::AGGREGATION_MAX_FAILURES;
use super::actors::calculate_full_jitter_backoff;

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
    let Some(players) = state.get("player_telemetry").and_then(Value::as_object) else {
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

    let Some(players) = state.get("player_telemetry").and_then(Value::as_object) else {
        return (Some(wt), None);
    };

    let max_score = players
        .values()
        .filter(|p| extract_team(p) == Some(wt))
        .filter_map(|p| p.get("score").and_then(Value::as_i64))
        .max();

    (Some(wt), max_score)
}

#[expect(
    clippy::too_many_lines,
    reason = "Aggregation orchestrates match index, player logs, cumulative player stats, and cumulative team stats in a single pass."
)]
pub async fn upload_aggregation(
    sink: &Arc<dyn TelemetrySink + Send + Sync>,
    match_id: &str,
    state: &Value,
    shutdown: &CancellationToken,
) {
    let index_entry = build_match_index_entry(match_id, state);
    let player_logs = build_player_match_logs(match_id, state);
    let (winning_team, winning_team_max_score) = compute_match_outcome(state, &index_entry);

    let mut blue_ids: Vec<String> = Vec::new();
    let mut orange_ids: Vec<String> = Vec::new();
    let mut blue_players: Vec<&Value> = Vec::new();
    let mut orange_players: Vec<&Value> = Vec::new();

    let mut upload_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>> =
        Vec::new();

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

    if let Some(players) = state.get("player_telemetry").and_then(Value::as_object) {
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

            if let Some((_, log)) = player_logs.iter().find(|(sid, _)| sid == &sanitized_id) {
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

    // --- Team cumulative stats ---
    if let Some(players) = state.get("player_telemetry").and_then(Value::as_object) {
        for (player_id, player_data) in players {
            let sanitized_id = sanitize_firebase_key(player_id);
            match extract_team(player_data) {
                Some(0) => {
                    blue_ids.push(sanitized_id);
                    blue_players.push(player_data);
                }
                Some(1) => {
                    orange_ids.push(sanitized_id);
                    orange_players.push(player_data);
                }
                _ => {}
            }
        }
    }

    let (blue_team_id, orange_team_id) = join(
        resolve_team_id(sink, &blue_ids, match_id, "blue", shutdown),
        resolve_team_id(sink, &orange_ids, match_id, "orange", shutdown),
    )
    .await;

    let blue_won = winning_team == Some(0);
    if !blue_ids.is_empty() {
        upload_futures.push(Box::pin(update_cumulative_team_stats(
            Arc::clone(sink),
            blue_team_id,
            &blue_players,
            index_entry.blue_score,
            index_entry.orange_score,
            blue_won,
            shutdown.clone(),
        )));
    }

    if !orange_ids.is_empty() {
        upload_futures.push(Box::pin(update_cumulative_team_stats(
            Arc::clone(sink),
            orange_team_id,
            &orange_players,
            index_entry.orange_score,
            index_entry.blue_score,
            !blue_won,
            shutdown.clone(),
        )));
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
            Err(SinkError::RateLimited { .. } | SinkError::TransientNetwork { .. }) => {
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

async fn resolve_team_id(
    sink: &Arc<dyn TelemetrySink + Send + Sync>,
    sanitized_ids: &[String],
    match_id: &str,
    team_label: &str,
    shutdown: &CancellationToken,
) -> String {
    let mut fetch_futures: Vec<
        std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>,
    > = Vec::new();

    for id in sanitized_ids {
        let sink = Arc::clone(sink);
        let path = format!("players/{id}");
        let shutdown = shutdown.clone();

        fetch_futures.push(Box::pin(async move {
            if shutdown.is_cancelled() {
                return None;
            }
            match sink.get_node(&path).await {
                Ok(Some(value)) => serde_json::from_value::<PlayerRegistryEntry>(value)
                    .ok()
                    .and_then(|entry| entry.team_id),
                _ => None,
            }
        }));
    }

    let results: Vec<Option<String>> = join_all(fetch_futures).await;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for team_id in results.into_iter().flatten() {
        *counts.entry(team_id).or_default() += 1;
    }

    let max_entry = counts.iter().max_by_key(|(_, count)| *count);

    match max_entry {
        Some((team_id, &count)) if count > 0 => {
            let is_majority = counts
                .iter()
                .all(|(other_id, &other_count)| other_id == team_id || count > other_count);
            if is_majority {
                return team_id.clone();
            }
        }
        _ => {}
    }

    format!("temp_{match_id}_{team_label}")
}

#[expect(
    clippy::too_many_lines,
    reason = "Team R-M-W flow spans GET, stats summation, modify, and PUT with retries."
)]
async fn update_cumulative_team_stats(
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    team_id: String,
    team_player_data: &[&Value],
    goals_for: u64,
    goals_against: u64,
    won: bool,
    shutdown: CancellationToken,
) {
    let mut shots_total = 0_u64;
    let mut saves_total = 0_u64;
    let mut assists_total = 0_u64;
    let mut demos_total = 0_u64;

    for player_data in team_player_data {
        shots_total = shots_total.saturating_add(extract_u64(player_data, "shots"));
        saves_total = saves_total.saturating_add(extract_u64(player_data, "saves"));
        assists_total = assists_total.saturating_add(extract_u64(player_data, "assists"));
        demos_total = demos_total.saturating_add(extract_u64(player_data, "demos"));
    }

    let path = format!("stats_cumulative_teams/{team_id}");
    let mut failures = 0_u32;

    loop {
        if shutdown.is_cancelled() {
            return;
        }

        let cumulative = match sink.get_node(&path).await {
            Ok(Some(value)) => {
                serde_json::from_value::<CumulativeTeamStats>(value).unwrap_or_else(|err| {
                    warn!(
                        "Aggregation warning: failed to parse cumulative team stats for team_id={team_id}: {err}. Falling back to default."
                    );
                    CumulativeTeamStats::default()
                })
            }
            Ok(None) => CumulativeTeamStats::default(),
            Err(SinkError::Terminal { message }) => {
                error!(
                    "Aggregation error: terminal failure getting cumulative team stats for team_id={team_id}: {message}"
                );
                return;
            }
            Err(
                SinkError::RateLimited { .. } | SinkError::TransientNetwork { .. },
            ) => {
                failures = failures.saturating_add(1);
                if failures > AGGREGATION_MAX_FAILURES {
                    warn!(
                        "Aggregation warning: exceeded max failures ({AGGREGATION_MAX_FAILURES}) getting cumulative team stats for team_id={team_id}"
                    );
                    return;
                }

                let delay = calculate_full_jitter_backoff(failures);
                warn!(
                    "Aggregation warning: retrying get for team_id={team_id} failures={failures} retrying_in_ms={}.",
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
        cumulative.matches_played = cumulative.matches_played.saturating_add(1);
        if won {
            cumulative.wins = cumulative.wins.saturating_add(1);
        } else {
            cumulative.losses = cumulative.losses.saturating_add(1);
        }
        cumulative.goals_for = cumulative.goals_for.saturating_add(goals_for);
        cumulative.goals_against = cumulative.goals_against.saturating_add(goals_against);
        cumulative.shots = cumulative.shots.saturating_add(shots_total);
        cumulative.saves = cumulative.saves.saturating_add(saves_total);
        cumulative.assists = cumulative.assists.saturating_add(assists_total);
        cumulative.demos = cumulative.demos.saturating_add(demos_total);

        let data = match serde_json::to_value(&cumulative) {
            Ok(v) => v,
            Err(err) => {
                error!(
                    "Aggregation error: failed to serialize cumulative team stats for team_id={team_id}: {err}"
                );
                return;
            }
        };

        match sink.put_node(&path, &data).await {
            Ok(()) => return,
            Err(SinkError::Terminal { message }) => {
                error!(
                    "Aggregation error: terminal failure putting cumulative team stats for team_id={team_id}: {message}"
                );
                return;
            }
            Err(SinkError::RateLimited { .. } | SinkError::TransientNetwork { .. }) => {
                failures = failures.saturating_add(1);
                if failures > AGGREGATION_MAX_FAILURES {
                    warn!(
                        "Aggregation warning: exceeded max failures ({AGGREGATION_MAX_FAILURES}) putting cumulative team stats for team_id={team_id}"
                    );
                    return;
                }

                let delay = calculate_full_jitter_backoff(failures);
                warn!(
                    "Aggregation warning: retrying put for team_id={team_id} failures={failures} retrying_in_ms={}.",
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
                error!("Aggregation error: terminal failure putting node={path}: {message}");
                return;
            }
            Err(SinkError::RateLimited { .. } | SinkError::TransientNetwork { .. }) => {
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
        let stats: CumulativePlayerStats = serde_json::from_value(json).unwrap_or_default();
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

    #[test]
    fn player_registry_deserializes_with_team_id() {
        let json = json!({"team_id": "EG"});
        let entry: PlayerRegistryEntry = serde_json::from_value(json).unwrap_or_default();
        assert_eq!(entry.team_id, Some("EG".to_string()));
    }

    #[test]
    fn player_registry_deserializes_missing_team_id() {
        let json = json!({});
        let entry: PlayerRegistryEntry = serde_json::from_value(json).unwrap_or_default();
        assert_eq!(entry.team_id, None);
    }

    #[test]
    fn player_registry_deserializes_with_unknown_fields() {
        let json = json!({"team_id": "NRG", "rank": "Diamond"});
        let entry: PlayerRegistryEntry = serde_json::from_value(json).unwrap_or_default();
        assert_eq!(entry.team_id, Some("NRG".to_string()));
    }

    #[test]
    fn cumulative_team_stats_default_is_all_zeros() {
        let stats = CumulativeTeamStats::default();
        assert_eq!(stats.matches_played, 0);
        assert_eq!(stats.wins, 0);
        assert_eq!(stats.losses, 0);
        assert_eq!(stats.goals_for, 0);
        assert_eq!(stats.goals_against, 0);
        assert_eq!(stats.shots, 0);
        assert_eq!(stats.saves, 0);
        assert_eq!(stats.assists, 0);
        assert_eq!(stats.demos, 0);
    }

    #[test]
    fn cumulative_team_stats_deserializes_partial_json() {
        let json = json!({"wins": 5, "goals_for": 12});
        let stats: CumulativeTeamStats = serde_json::from_value(json).unwrap_or_default();
        assert_eq!(stats.wins, 5);
        assert_eq!(stats.goals_for, 12);
        assert_eq!(stats.matches_played, 0);
        assert_eq!(stats.losses, 0);
    }
}
