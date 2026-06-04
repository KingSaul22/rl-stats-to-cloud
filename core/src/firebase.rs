use crate::connector::{EventSink, SinkError};
use crate::firebase_auth::FirebaseAuth;
use async_trait::async_trait;
use reqwest::Client;
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct FirebaseConnector {
    base_url: String,
    auth: FirebaseAuth,
    http: Client,
}

impl FirebaseConnector {
    /// Creates a Firebase connector and performs an initial authentication login.
    ///
    /// # Errors
    /// Returns an error if Firebase authentication login fails.
    pub async fn new(
        firebase_url: impl Into<String>,
        api_key: String,
        email: String,
        password: String,
    ) -> Result<Self, crate::firebase_auth::AuthError> {
        let base_url = firebase_url.into().trim_end_matches('/').to_string();
        let http = match Client::builder().timeout(Duration::from_secs(5)).build() {
            Ok(client) => client,
            Err(err) => {
                eprintln!(
                    "Firebase connector warning: failed to build timed HTTP client ({err}). Falling back to default reqwest client."
                );
                Client::new()
            }
        };
        let auth = FirebaseAuth::new(api_key, email, password);
        auth.login().await?;

        Ok(Self {
            base_url,
            auth,
            http,
        })
    }

    async fn push_event(&self, event_type: &str, payload: &Value) -> Result<(), SinkError> {
        let match_id = payload
            .get("match_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown_match");
        let _session_id = payload.get("session_id").and_then(Value::as_str);

        let route = FirebaseRoute::from_event_type(event_type);
        let endpoint = route.endpoint_path(match_id);
        let auth_token = self.auth.get_token().await.map_err(|err| {
            let message = format!("firebase auth token retrieval failed: {err}");
            eprintln!("Firebase push warning: {message}");
            SinkError::transient(message)
        })?;

        let url = self.build_json_url(&endpoint, &auth_token);
        let redacted_url = Self::redact_url(&url);

        let request = match route {
            FirebaseRoute::LiveState => self.http.put(&url),
            FirebaseRoute::EventFeed | FirebaseRoute::Historical => self.http.post(&url),
        };

        let response = request.json(payload).send().await.map_err(|err| {
            let mapped = Self::map_reqwest_error(&err);
            let err_message = Self::redact_message(&err.to_string());
            eprintln!("Firebase push warning: failed to send to {redacted_url} ({err_message})");
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

    /// Deletes a JSON node in Firebase Realtime Database.
    ///
    /// # Errors
    /// Returns an error when authentication fails, the request cannot be sent,
    /// or Firebase returns a non-success status code.
    pub async fn delete_node(&self, path: &str) -> Result<(), SinkError> {
        let auth_token = self.auth.get_token().await.map_err(|err| {
            let message = format!("firebase auth token retrieval failed: {err}");
            eprintln!("Firebase delete warning: {message}");
            SinkError::transient(message)
        })?;

        let normalized_path = path.trim_matches('/');
        let url = self.build_json_url(normalized_path, &auth_token);
        let redacted_url = Self::redact_url(&url);

        let response = self.http.delete(&url).send().await.map_err(|err| {
            let mapped = Self::map_reqwest_error(&err);
            let err_message = Self::redact_message(&err.to_string());
            eprintln!("Firebase delete warning: failed to send to {redacted_url} ({err_message})");
            mapped
        })?;

        if !response.status().is_success() {
            let mapped = Self::map_status_error(response.status(), &redacted_url);
            eprintln!(
                "Firebase delete warning: {} returned status {}",
                redacted_url,
                response.status()
            );
            return Err(mapped);
        }

        Ok(())
    }

    fn build_json_url(&self, endpoint_path: &str, auth_token: &str) -> String {
        format!("{}/{}.json?auth={auth_token}", self.base_url, endpoint_path)
    }

    fn redact_url(url: &str) -> String {
        url.find("auth=").map_or_else(
            || url.to_string(),
            |start| {
                let token_start = start + "auth=".len();
                let token_end = url[token_start..]
                    .find('&')
                    .map_or(url.len(), |index| token_start + index);

                let mut redacted = url.to_string();
                redacted.replace_range(token_start..token_end, "[REDACTED]");
                redacted
            },
        )
    }

    fn redact_message(message: &str) -> String {
        Self::redact_url(message)
    }

    fn map_reqwest_error(err: &reqwest::Error) -> SinkError {
        if let Some(status) = err.status() {
            return Self::map_status_error(status, &Self::redact_message(&err.to_string()));
        }

        let redacted = Self::redact_message(&err.to_string());
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

    async fn delete_node(&self, path: &str) -> Result<(), SinkError> {
        Self::delete_node(self, path).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirebaseRoute {
    LiveState,
    EventFeed,
    Historical,
}

impl FirebaseRoute {
    fn from_event_type(event_type: &str) -> Self {
        if matches!(
            event_type,
            "UpdateState" | "ClockUpdated" | "ClockUpdatedSeconds"
        ) {
            Self::LiveState
        } else if matches!(event_type, "Goal" | "GoalScored" | "Save" | "Demolition") {
            Self::Historical
        } else {
            Self::EventFeed
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

    #[test]
    fn from_event_type_maps_live_state_events_to_live_state() {
        for event_type in ["UpdateState", "ClockUpdated"] {
            assert_eq!(
                FirebaseRoute::from_event_type(event_type),
                FirebaseRoute::LiveState
            );
        }
    }

    #[test]
    fn from_event_type_maps_historical_events_to_historical() {
        for event_type in ["Goal", "GoalScored", "Save", "Demolition"] {
            assert_eq!(
                FirebaseRoute::from_event_type(event_type),
                FirebaseRoute::Historical
            );
        }
    }

    #[test]
    fn from_event_type_falls_back_to_event_feed_for_unknown_events() {
        assert_eq!(
            FirebaseRoute::from_event_type("SomeUnknownGameEvent"),
            FirebaseRoute::EventFeed
        );
    }

    #[test]
    fn endpoint_path_formats_historical_route_with_match_id() {
        assert_eq!(
            FirebaseRoute::Historical.endpoint_path("dummy_match_id"),
            "matches_events_history/dummy_match_id"
        );
    }

    #[test]
    fn endpoint_path_formats_event_feed_route() {
        assert_eq!(
            FirebaseRoute::EventFeed.endpoint_path("ignored_match"),
            "live_events_feed"
        );
    }

    #[test]
    fn redact_url_masks_delete_auth_token() {
        let url = "https://example.firebaseio.com/live_state.json?auth=sensitive_token";
        assert_eq!(
            FirebaseConnector::redact_url(url),
            "https://example.firebaseio.com/live_state.json?auth=[REDACTED]"
        );
    }
}
