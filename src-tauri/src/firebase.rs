use reqwest::Client;
use serde_json::Value;

#[derive(Clone)]
pub struct FirebaseClient {
    base_url: String,
    auth_token: Option<String>,
    http: Client,
}

impl FirebaseClient {
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

    pub async fn push_event(&self, event_type: &str, payload: &Value) {
        let route = FirebaseRoute::from_event_type(event_type);
        let endpoint = match route {
            FirebaseRoute::LiveState => "live_state".to_string(),
            FirebaseRoute::MatchEvent => {
                let safe_event_type = sanitize_event_type(event_type);
                format!("match_events/{}", safe_event_type)
            }
        };

        let url = self.build_json_url(&endpoint);
        let redacted_url = self.redact_url(&url);

        let request = match route {
            FirebaseRoute::LiveState => self.http.put(&url),
            FirebaseRoute::MatchEvent => self.http.post(&url),
        };

        let response = request.json(payload).send().await;
        match response {
            Ok(resp) => {
                if !resp.status().is_success() {
                    eprintln!(
                        "Firebase push warning: {} returned status {}",
                        redacted_url,
                        resp.status()
                    );
                }
            }
            Err(err) => {
                let err_message = self.redact_message(&err.to_string());
                eprintln!(
                    "Firebase push warning: failed to send to {} ({})",
                    redacted_url,
                    err_message
                );
            }
        }
    }

    fn build_json_url(&self, endpoint_path: &str) -> String {
        let mut url = format!("{}/{}.json", self.base_url, endpoint_path);
        if let Some(token) = &self.auth_token {
            url.push_str("?auth=");
            url.push_str(token);
        }
        url
    }

    fn redact_url(&self, url: &str) -> String {
        if let Some(start) = url.find("auth=") {
            let token_start = start + "auth=".len();
            let token_end = url[token_start..]
                .find('&')
                .map(|index| token_start + index)
                .unwrap_or(url.len());

            let mut redacted = url.to_string();
            redacted.replace_range(token_start..token_end, "[REDACTED]");
            redacted
        } else {
            url.to_string()
        }
    }

    fn redact_message(&self, message: &str) -> String {
        let mut output = message.to_string();
        if let Some(token) = &self.auth_token {
            output = output.replace(token, "[REDACTED]");
        }

        output
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
            "UpdateState" | "ClockUpdatedSeconds" => FirebaseRoute::LiveState,
            _ => FirebaseRoute::MatchEvent,
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
