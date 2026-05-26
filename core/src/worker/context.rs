use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SessionContext {
    pub(crate) active_match_id: String,
    pub(crate) active_session_id: String,
    pub(crate) id_source: String,
}

impl SessionContext {
    #[must_use]
    pub(crate) fn new(config_match_id: Option<String>, config_session_id: Option<String>) -> Self {
        let mut context = Self {
            active_match_id: String::new(),
            active_session_id: String::new(),
            id_source: "Generated".to_string(),
        };

        if let Some(match_id) = config_match_id
            && !match_id.is_empty()
        {
            context.active_match_id = match_id;
            context.id_source = "Config".to_string();
        }

        if let Some(session_id) = config_session_id
            && !session_id.is_empty()
        {
            context.active_session_id = session_id;
            context.id_source = "Config".to_string();
        }

        context.ensure_fallback_ids();
        context
    }

    pub(crate) fn update_from_payload(&mut self, payload: &Value) {
        const MATCH_KEYS: &[&str] = &[
            "match_id", "matchId", "MatchID", "MatchId", "game_id", "gameId", "GameID",
        ];
        const SESSION_KEYS: &[&str] = &[
            "session_id",
            "sessionId",
            "SessionID",
            "SessionId",
            "game_session_id",
            "gameSessionId",
            "GameSessionID",
        ];

        let telemetry_match_id = Self::extract_identifier(payload, MATCH_KEYS);
        let telemetry_session_id = Self::extract_identifier(payload, SESSION_KEYS);

        if let Some(match_id) = telemetry_match_id
            && !match_id.is_empty()
        {
            self.active_match_id = match_id;
            self.id_source = "Telemetry".to_string();
        }

        if let Some(session_id) = telemetry_session_id
            && !session_id.is_empty()
        {
            self.active_session_id = session_id;
            self.id_source = "Telemetry".to_string();
        }

        self.ensure_fallback_ids();
    }

    pub(crate) fn ensure_fallback_ids(&mut self) {
        if self.active_match_id.is_empty() {
            self.active_match_id = Self::generate_fallback_id("match");
            if self.id_source != "Config" {
                self.id_source = "Generated".to_string();
            }
        }

        if self.active_session_id.is_empty() {
            self.active_session_id = Self::generate_fallback_id("session");
            if self.id_source != "Config" {
                self.id_source = "Generated".to_string();
            }
        }
    }

    #[must_use]
    pub(crate) fn extract_identifier(payload: &Value, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| payload.get(key).and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    #[must_use]
    pub(crate) fn generate_fallback_id(prefix: &str) -> String {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0_u128, |duration| duration.as_millis());
        format!("{prefix}_{timestamp_ms}")
    }
}
