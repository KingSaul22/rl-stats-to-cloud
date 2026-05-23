use crate::connector::{EventSink, SinkError};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct FirebaseConnector {
    base_url: String,
    auth_token: Option<String>,
    http: Client,
}

impl FirebaseConnector {
    pub fn new(firebase_url: impl Into<String>, firebase_auth_token: Option<String>) -> Self {
        let base_url = firebase_url.into().trim_end_matches('/').to_string();
        let auth_token = firebase_auth_token
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
        let http = match Client::builder().timeout(Duration::from_secs(5)).build() {
            Ok(client) => client,
            Err(err) => {
                eprintln!(
                    "Firebase connector warning: failed to build timed HTTP client ({err}). Falling back to default reqwest client."
                );
                Client::new()
            }
        };

        Self {
            base_url,
            auth_token,
            http,
        }
    }

    async fn push_event(&self, event_type: &str, payload: &Value) -> Result<(), SinkError> {
        let match_id = payload
            .get("match_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown_match");
        let _session_id = payload.get("session_id").and_then(Value::as_str);

        let route = FirebaseRoute::from_event_type(event_type);
        let endpoint = route.endpoint_path(match_id);

        let url = self.build_json_url(&endpoint);
        let redacted_url = Self::redact_url(&url);

        let request = match route {
            FirebaseRoute::LiveState => self.http.put(&url),
            FirebaseRoute::EventFeed | FirebaseRoute::Historical => self.http.post(&url),
        };

        let response = request.json(payload).send().await.map_err(|err| {
            let mapped = self.map_reqwest_error(&err);
            let err_message = self.redact_message(&err.to_string());
            eprintln!(
                "Firebase push warning: failed to send to {redacted_url} ({err_message})"
            );
            mapped
        })?;

        if !response.status().is_success() {
            let mapped = Self::map_status_error(response.status(), &redacted_url);
            eprintln!(
                "Firebase push warning: {} returned status {}",
                redacted_url,
                response.status()
            );
            return Err(mapped);
        }

        Ok(())
    }

    fn build_json_url(&self, endpoint_path: &str) -> String {
        let mut url = format!("{}/{}.json", self.base_url, endpoint_path);
        if let Some(token) = &self.auth_token {
            url.push_str("?auth=");
            url.push_str(token);
        }
        url
    }

    fn redact_url(url: &str) -> String {
        url.find("auth=").map_or_else(|| url.to_string(), |start| {
            let token_start = start + "auth=".len();
            let token_end = url[token_start..]
                .find('&')
                .map_or(url.len(), |index| token_start + index);

            let mut redacted = url.to_string();
            redacted.replace_range(token_start..token_end, "[REDACTED]");
            redacted
        })
    }

    fn redact_message(&self, message: &str) -> String {
        let mut output = message.to_string();
        if let Some(token) = &self.auth_token {
            output = output.replace(token, "[REDACTED]");
        }

        output
    }

    fn map_reqwest_error(&self, err: &reqwest::Error) -> SinkError {
        if let Some(status) = err.status() {
            return Self::map_status_error(status, &self.redact_message(&err.to_string()));
        }

        let redacted = self.redact_message(&err.to_string());
        if err.is_timeout() || err.is_connect() || err.is_request() {
            SinkError::transient(redacted)
        } else {
            SinkError::terminal(redacted)
        }
    }

    fn map_status_error(status: StatusCode, context: &str) -> SinkError {
        let message = format!("{context} (status {status})");
        if status == StatusCode::TOO_MANY_REQUESTS {
            SinkError::rate_limited(message)
        } else if status.is_server_error() {
            SinkError::transient(message)
        } else {
            SinkError::terminal(message)
        }
    }
}

#[async_trait]
impl EventSink for FirebaseConnector {
    async fn send_event(&self, event_type: &str, payload: &Value) -> Result<(), SinkError> {
        self.push_event(event_type, payload).await
    }
}

#[derive(Debug, Clone, Copy)]
enum FirebaseRoute {
    LiveState,
    EventFeed,
    Historical,
}

impl FirebaseRoute {
    fn from_event_type(event_type: &str) -> Self {
        match event_type {
            "UpdateState" | "ClockUpdated" | "ClockUpdatedSeconds" => Self::LiveState,
            "EventFeedMarker" | "MatchHistoryMarker" => Self::EventFeed,
            "Goal" | "GoalScored" | "Save" | "EpicSave" | "Demolition" | "Demo" => {
                Self::Historical
            }
            _ => Self::Historical,
        }
    }

    fn endpoint_path(self, match_id: &str) -> String {
        match self {
            Self::LiveState => "live_state".to_string(),
            Self::EventFeed => "live_events_feed".to_string(),
            Self::Historical => format!("matches_events_history/{match_id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn taxonomy_maps_clock_variants_to_live_state_route() {
        for event_type in ["UpdateState", "ClockUpdated", "ClockUpdatedSeconds"] {
            assert!(matches!(
                FirebaseRoute::from_event_type(event_type),
                FirebaseRoute::LiveState
            ));
        }
    }

    #[test]
    fn historical_route_falls_back_to_unknown_match_path() {
        assert_eq!(
            FirebaseRoute::Historical.endpoint_path("unknown_match"),
            "matches_events_history/unknown_match"
        );
    }

    #[test]
    fn event_feed_routes_to_live_events_feed_path() {
        assert_eq!(
            FirebaseRoute::EventFeed.endpoint_path("ignored_match"),
            "live_events_feed"
        );
    }

    #[test]
    fn route_selection_ignores_missing_match_id_for_unknown_match_fallback_shape() {
        let payload = json!({
            "session_id": "session_123"
        });

        let match_id = payload
            .get("match_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown_match");

        assert_eq!(match_id, "unknown_match");
        assert_eq!(
            FirebaseRoute::Historical.endpoint_path(match_id),
            "matches_events_history/unknown_match"
        );
    }
}
