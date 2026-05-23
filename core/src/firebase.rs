use crate::connector::{EventSink, SinkError};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::StatusCode;
use serde_json::Value;

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

        Self {
            base_url,
            auth_token,
            http: Client::new(),
        }
    }

    async fn push_event(&self, event_type: &str, payload: &Value) -> Result<(), SinkError> {
        let route = FirebaseRoute::from_event_type(event_type);
        let endpoint = match route {
            FirebaseRoute::LiveState => "live_state".to_string(),
            FirebaseRoute::MatchEvent => {
                let safe_event_type = sanitize_event_type(event_type);
                format!("match_events/{safe_event_type}")
            }
        };

        let url = self.build_json_url(&endpoint);
        let redacted_url = Self::redact_url(&url);

        let request = match route {
            FirebaseRoute::LiveState => self.http.put(&url),
            FirebaseRoute::MatchEvent => self.http.post(&url),
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
    MatchEvent,
}

impl FirebaseRoute {
    fn from_event_type(event_type: &str) -> Self {
        match event_type {
            "UpdateState" | "ClockUpdatedSeconds" => Self::LiveState,
            _ => Self::MatchEvent,
        }
    }
}

fn sanitize_event_type(event_type: &str) -> String {
    let mut out = String::with_capacity(event_type.len());

    for ch in event_type.chars() {
        match ch {
            '.' | '$' | '#' | '[' | ']' | '/' => out.push('_'),
            _ => out.push(ch),
        }
    }

    if out.is_empty() {
        "unknown_event".to_string()
    } else {
        out
    }
}
