use super::context::SessionContext;
use super::events::IngestClass;
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedPayloadKind {
	LiveState,
	EventFeed,
	Historical,
}

pub fn normalize_payload(
	class: IngestClass,
	raw: &Value,
	event_type: &str,
	session_context: &SessionContext,
) -> Value {
	match normalized_payload_kind(class) {
		NormalizedPayloadKind::LiveState => normalize_live_state(raw, session_context),
		NormalizedPayloadKind::EventFeed => normalize_event_feed(raw, event_type, session_context),
		NormalizedPayloadKind::Historical => normalize_historical(raw, event_type, session_context),
	}
}

pub const fn normalized_payload_kind(class: IngestClass) -> NormalizedPayloadKind {
	match class {
		IngestClass::LiveState => NormalizedPayloadKind::LiveState,
		IngestClass::EventFeed => NormalizedPayloadKind::EventFeed,
		IngestClass::Historical => NormalizedPayloadKind::Historical,
	}
}

pub fn normalize_live_state(raw: &Value, session_context: &SessionContext) -> Value {
	let time_remaining_seconds = extract_u64_from_keys(
		raw,
		&[
			"time_remaining_seconds",
			"timeRemainingSeconds",
			"seconds_remaining",
			"secondsRemaining",
			"remaining_seconds",
			"remainingSeconds",
			"clock_seconds_remaining",
			"clockSecondsRemaining",
		],
	)
	.unwrap_or(0);

	let score = extract_score_object(raw);
	let player_telemetry = extract_player_telemetry(raw);

	let mut payload = Map::new();
	payload.insert("is_active".to_string(), Value::Bool(true));
	payload.insert(
		"session_id".to_string(),
		Value::String(session_context.active_session_id.clone()),
	);
	payload.insert(
		"match_id".to_string(),
		Value::String(session_context.active_match_id.clone()),
	);
	payload.insert(
		"time_remaining_seconds".to_string(),
		Value::from(time_remaining_seconds),
	);
	payload.insert("score".to_string(), score);
	payload.insert("player_telemetry".to_string(), player_telemetry);
	Value::Object(payload)
}

pub fn normalize_event_feed(
	raw: &Value,
	event_type: &str,
	session_context: &SessionContext,
) -> Value {
	let mut payload = Map::new();
	payload.insert("timestamp_ms".to_string(), Value::from(current_timestamp_ms()));
	payload.insert(
		"game_seconds_remaining".to_string(),
		Value::from(extract_game_seconds_remaining(raw).unwrap_or(0)),
	);
	payload.insert("type".to_string(), Value::String(canonical_event_type(event_type)));
	payload.insert(
		"match_id".to_string(),
		Value::String(session_context.active_match_id.clone()),
	);
	payload.insert(
		"session_id".to_string(),
		Value::String(session_context.active_session_id.clone()),
	);

	if let Some(attacker_id) = extract_string_from_keys(
		raw,
		&[
			"attacker_id",
			"attackerId",
			"player_id",
			"playerId",
			"scorer_id",
			"scorerId",
			"actor_id",
			"actorId",
		],
	) {
		payload.insert("attacker_id".to_string(), Value::String(attacker_id));
	}

	if let Some(victim_id) = extract_string_from_keys(
		raw,
		&[
			"victim_id",
			"victimId",
			"target_id",
			"targetId",
			"defender_id",
			"defenderId",
		],
	) {
		payload.insert("victim_id".to_string(), Value::String(victim_id));
	}

	Value::Object(payload)
}

pub fn normalize_historical(
	raw: &Value,
	event_type: &str,
	session_context: &SessionContext,
) -> Value {
	let mut payload = Map::new();
	payload.insert(
		"timestamp_ms".to_string(),
		Value::from(extract_timestamp_ms(raw).unwrap_or_else(current_timestamp_ms)),
	);
	payload.insert(
		"game_seconds_remaining".to_string(),
		Value::from(extract_game_seconds_remaining(raw).unwrap_or(0)),
	);
	payload.insert("type".to_string(), Value::String(canonical_event_type(event_type)));
	payload.insert(
		"match_id".to_string(),
		Value::String(session_context.active_match_id.clone()),
	);
	payload.insert(
		"session_id".to_string(),
		Value::String(session_context.active_session_id.clone()),
	);

	if let Some(player_id) = extract_string_from_keys(
		raw,
		&[
			"player_id",
			"playerId",
			"attacker_id",
			"attackerId",
			"scorer_id",
			"scorerId",
			"actor_id",
			"actorId",
		],
	) {
		payload.insert("player_id".to_string(), Value::String(player_id));
	}

	let details = extract_details_object(raw);
	payload.insert("details".to_string(), details);
	Value::Object(payload)
}

pub fn canonical_event_type(event_type: &str) -> String {
	match event_type {
		"UpdateState" | "ClockUpdated" | "ClockUpdatedSeconds" => "live_state".to_string(),
		"Goal" | "GoalScored" => "goal".to_string(),
		"Save" | "EpicSave" => "save".to_string(),
		"Demolition" | "Demo" => "demo".to_string(),
		other => other.to_ascii_lowercase(),
	}
}

pub fn current_timestamp_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0_u128, |duration| duration.as_millis())
		.try_into()
		.map_or(u64::MAX, |value| value)
}

pub fn extract_timestamp_ms(raw: &Value) -> Option<u64> {
	extract_u64_from_keys(
		raw,
		&[
			"timestamp_ms",
			"timestampMs",
			"TimestampMs",
			"timestamp",
			"Timestamp",
		],
	)
}

pub fn extract_game_seconds_remaining(raw: &Value) -> Option<u64> {
	extract_u64_from_keys(
		raw,
		&[
			"game_seconds_remaining",
			"gameSecondsRemaining",
			"seconds_remaining",
			"secondsRemaining",
			"time_remaining_seconds",
			"timeRemainingSeconds",
			"remaining_seconds",
			"remainingSeconds",
		],
	)
}

pub fn extract_score_object(raw: &Value) -> Value {
	let blue = extract_u64_from_keys(
		raw,
		&["blue", "blue_score", "blueScore", "score_blue", "scoreBlue"],
	)
	.unwrap_or(0);
	let orange = extract_u64_from_keys(
		raw,
		&[
			"orange",
			"orange_score",
			"orangeScore",
			"score_orange",
			"scoreOrange",
		],
	)
	.unwrap_or(0);

	let mut score = Map::new();
	score.insert("blue".to_string(), Value::from(blue));
	score.insert("orange".to_string(), Value::from(orange));
	Value::Object(score)
}

pub fn extract_player_telemetry(raw: &Value) -> Value {
	let mut players = Map::new();
	collect_player_telemetry(raw, &mut players, None);
	Value::Object(players)
}

pub fn collect_player_telemetry(
	raw: &Value,
	players: &mut Map<String, Value>,
	parent_key: Option<&str>,
) {
	match raw {
		Value::Object(object) => {
			let player_id = extract_string_from_keys(
				raw,
				&[
					"player_id",
					"playerId",
					"id",
					"Id",
					"unique_id",
					"uniqueId",
					"steam_id",
					"steamId",
					"epic_id",
					"epicId",
				],
			)
			.or_else(|| parent_key.map(ToString::to_string));

			let mut telemetry = Map::new();

			if let Some(boost) = extract_u64_from_keys(raw, &["boost", "Boost"]) {
				telemetry.insert("boost".to_string(), Value::from(boost));
			}
			if let Some(score) = extract_i64_from_keys(raw, &["score", "Score"]) {
				telemetry.insert("score".to_string(), Value::from(score));
			}
			if let Some(goals) = extract_u64_from_keys(raw, &["goals", "Goals"]) {
				telemetry.insert("goals".to_string(), Value::from(goals));
			}

			if !telemetry.is_empty()
				&& let Some(player_id) = player_id
			{
				players.insert(player_id, Value::Object(telemetry));
			}

			for (key, value) in object {
				collect_player_telemetry(value, players, Some(key));
			}
		}
		Value::Array(values) => {
			for value in values {
				collect_player_telemetry(value, players, parent_key);
			}
		}
		_ => {}
	}
}

pub fn extract_details_object(raw: &Value) -> Value {
	if let Some(details) = raw.get("details")
		&& details.is_object()
	{
		return details.clone();
	}

	let mut details = Map::new();

	if let Some(speed_kph) = extract_u64_from_keys(raw, &["speed_kph", "speedKph"]) {
		details.insert("speed_kph".to_string(), Value::from(speed_kph));
	}

	Value::Object(details)
}

pub fn extract_string_from_keys(raw: &Value, keys: &[&str]) -> Option<String> {
	find_value_by_keys(raw, keys)
		.and_then(Value::as_str)
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(ToString::to_string)
}

pub fn extract_u64_from_keys(raw: &Value, keys: &[&str]) -> Option<u64> {
	find_value_by_keys(raw, keys).and_then(|value| {
		value
			.as_u64()
			.or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
			.or_else(|| value.as_str().and_then(|text| text.trim().parse::<u64>().ok()))
	})
}

pub fn extract_i64_from_keys(raw: &Value, keys: &[&str]) -> Option<i64> {
	find_value_by_keys(raw, keys).and_then(|value| {
		value
			.as_i64()
			.or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
			.or_else(|| value.as_str().and_then(|text| text.trim().parse::<i64>().ok()))
	})
}

pub fn find_value_by_keys<'a>(raw: &'a Value, keys: &[&str]) -> Option<&'a Value> {
	match raw {
		Value::Object(object) => {
			for key in keys {
				if let Some(value) = object.get(*key) {
					return Some(value);
				}
			}

			object
				.values()
				.find_map(|value| find_value_by_keys(value, keys))
		}
		Value::Array(values) => values.iter().find_map(|value| find_value_by_keys(value, keys)),
		_ => None,
	}
}
