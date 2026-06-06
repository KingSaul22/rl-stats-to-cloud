use reqwest::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use tokio::sync::RwLock;

const LOGIN_ENDPOINT: &str =
    "https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword";
const REFRESH_ENDPOINT: &str = "https://securetoken.googleapis.com/v1/token";
const REFRESH_MARGIN: Duration = Duration::from_mins(5);

#[derive(Debug, Clone)]
pub struct TokenState {
    pub id_token: String,
    pub refresh_token: String,
    pub expires_at: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthRuntimeState {
    MissingCredentials,
    Unauthenticated,
    Authenticated,
}

impl Default for TokenState {
    fn default() -> Self {
        Self {
            id_token: String::new(),
            refresh_token: String::new(),
            expires_at: SystemTime::UNIX_EPOCH,
        }
    }
}

#[derive(Clone)]
pub struct FirebaseAuth {
    api_key: String,
    email: String,
    password: Arc<RwLock<Option<String>>>,
    http: Client,
    state: Arc<RwLock<TokenState>>,
    runtime_state: Arc<RwLock<AuthRuntimeState>>,
}

impl FirebaseAuth {
    #[must_use]
    pub fn new(api_key: String, email: String, password: Option<String>) -> Self {
        let http = match Client::builder().timeout(Duration::from_secs(5)).build() {
            Ok(client) => client,
            Err(err) => {
                eprintln!(
                    "Firebase auth warning: failed to build timed HTTP client ({err}). Falling back to default reqwest client."
                );
                Client::new()
            }
        };

        let normalized_password = normalize_password(password);
        let runtime_state = if normalized_password.is_some() {
            AuthRuntimeState::Unauthenticated
        } else {
            AuthRuntimeState::MissingCredentials
        };

        Self {
            api_key,
            email,
            password: Arc::new(RwLock::new(normalized_password)),
            http,
            state: Arc::new(RwLock::new(TokenState::default())),
            runtime_state: Arc::new(RwLock::new(runtime_state)),
        }
    }

    /// Update the in-memory password and immediately attempt authentication.
    ///
    /// # Errors
    /// Returns an error if provided credentials are empty or rejected by Firebase.
    pub async fn update_credentials(&self, password: String) -> Result<(), AuthError> {
        {
            let mut password_guard = self.password.write().await;
            *password_guard = normalize_password(Some(password));
        }

        {
            let mut state = self.state.write().await;
            *state = TokenState::default();
        }

        {
            let mut runtime_state = self.runtime_state.write().await;
            *runtime_state = AuthRuntimeState::Unauthenticated;
        }

        self.login().await
    }

    pub async fn runtime_state(&self) -> AuthRuntimeState {
        *self.runtime_state.read().await
    }

    /// Authenticate with Firebase Identity Toolkit and cache ID/refresh tokens.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails, Firebase rejects credentials, or response payload is invalid.
    pub async fn login(&self) -> Result<(), AuthError> {
        let password = {
            let password_guard = self.password.read().await;
            if let Some(password) = password_guard.clone() {
                password
            } else {
                let mut runtime_state = self.runtime_state.write().await;
                *runtime_state = AuthRuntimeState::MissingCredentials;
                drop(runtime_state);
                return Err(AuthError::MissingCredentials);
            }
        };

        let url = format!("{LOGIN_ENDPOINT}?key={}", self.api_key);
        let payload = SignInRequest {
            email: self.email.clone(),
            password,
            return_secure_token: true,
        };

        let response = self
            .http
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(AuthError::Request)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(AuthError::Request)?;
            return Err(AuthError::HttpStatus {
                status,
                body: redact_error_body(&body),
            });
        }

        let parsed: SignInResponse = response.json().await.map_err(AuthError::Request)?;
        let expires_in_secs = parse_expires_in(&parsed.expires_in)?;
        let expires_at = compute_expires_at(expires_in_secs);

        let mut state = self.state.write().await;
        state.id_token = parsed.id_token;
        state.refresh_token = parsed.refresh_token;
        state.expires_at = expires_at;
        drop(state);

        let mut runtime_state = self.runtime_state.write().await;
        *runtime_state = AuthRuntimeState::Authenticated;
        drop(runtime_state);
        Ok(())
    }

    /// Returns a valid Firebase ID token, refreshing it when expiration is near.
    ///
    /// # Errors
    /// Returns an error when no valid token exists and refresh/login fails.
    pub async fn get_token(&self) -> Result<String, AuthError> {
        let should_refresh = {
            let state = self.state.read().await;
            state.id_token.is_empty() || token_needs_refresh(state.expires_at)
        };

        if should_refresh {
            self.refresh_token().await?;
        }

        let state = self.state.read().await;
        if state.id_token.is_empty() {
            let mut runtime_state = self.runtime_state.write().await;
            *runtime_state = AuthRuntimeState::Unauthenticated;
            drop(runtime_state);
            return Err(AuthError::MissingTokenState);
        }

        Ok(state.id_token.clone())
    }

    async fn refresh_token(&self) -> Result<(), AuthError> {
        if self.password.read().await.is_none() {
            let mut runtime_state = self.runtime_state.write().await;
            *runtime_state = AuthRuntimeState::MissingCredentials;
            drop(runtime_state);
            return Err(AuthError::MissingCredentials);
        }

        let existing_refresh_token = {
            let state = self.state.read().await;
            state.refresh_token.clone()
        };

        if existing_refresh_token.is_empty() {
            self.login().await?;
            return Ok(());
        }

        let url = format!("{REFRESH_ENDPOINT}?key={}", self.api_key);
        let payload = RefreshTokenRequest {
            grant_type: "refresh_token",
            refresh_token: existing_refresh_token,
        };

        let response = self
            .http
            .post(url)
            .form(&payload)
            .send()
            .await
            .map_err(AuthError::Request)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(AuthError::Request)?;
            return Err(AuthError::HttpStatus {
                status,
                body: redact_error_body(&body),
            });
        }

        let parsed: RefreshTokenResponse = response.json().await.map_err(AuthError::Request)?;
        let expires_in_secs = parse_expires_in(&parsed.expires_in)?;
        let expires_at = compute_expires_at(expires_in_secs);

        let mut state = self.state.write().await;
        state.id_token = parsed.id_token;
        state.refresh_token = parsed.refresh_token;
        state.expires_at = expires_at;
        drop(state);

        let mut runtime_state = self.runtime_state.write().await;
        *runtime_state = AuthRuntimeState::Authenticated;
        drop(runtime_state);
        Ok(())
    }
}

#[derive(Debug)]
pub enum AuthError {
    Request(reqwest::Error),
    HttpStatus { status: StatusCode, body: String },
    ParseExpiresIn(std::num::ParseIntError),
    MissingTokenState,
    MissingCredentials,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(err) => write!(f, "firebase auth request error: {err}"),
            Self::HttpStatus { status, body } => {
                write!(f, "firebase auth failed with status {status}: {body}")
            }
            Self::ParseExpiresIn(err) => write!(f, "invalid expires_in from firebase auth: {err}"),
            Self::MissingTokenState => write!(f, "firebase auth token state is missing"),
            Self::MissingCredentials => {
                write!(f, "firebase auth is missing credentials")
            }
        }
    }
}

impl Error for AuthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Request(err) => Some(err),
            Self::ParseExpiresIn(err) => Some(err),
            Self::HttpStatus { .. } | Self::MissingTokenState | Self::MissingCredentials => None,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInRequest {
    email: String,
    password: String,
    return_secure_token: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignInResponse {
    id_token: String,
    refresh_token: String,
    expires_in: String,
}

#[derive(Debug, Serialize)]
struct RefreshTokenRequest {
    grant_type: &'static str,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    id_token: String,
    refresh_token: String,
    expires_in: String,
}

fn parse_expires_in(expires_in: &str) -> Result<u64, AuthError> {
    expires_in.parse::<u64>().map_err(AuthError::ParseExpiresIn)
}

fn compute_expires_at(expires_in_secs: u64) -> SystemTime {
    SystemTime::now() + Duration::from_secs(expires_in_secs)
}

fn token_needs_refresh(expires_at: SystemTime) -> bool {
    expires_at
        .duration_since(SystemTime::now())
        .map_or(true, |remaining| remaining <= REFRESH_MARGIN)
}

fn redact_error_body(body: &str) -> String {
    body.to_string()
}

fn normalize_password(password: Option<String>) -> Option<String> {
    password.and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{REFRESH_MARGIN, token_needs_refresh};
    use std::time::Duration;
    use std::time::SystemTime;

    #[test]
    fn token_needs_refresh_when_already_expired() {
        let expires_at = SystemTime::now() - Duration::from_secs(1);
        assert!(token_needs_refresh(expires_at));
    }

    #[test]
    fn token_needs_refresh_when_within_margin() {
        let expires_at = SystemTime::now() + REFRESH_MARGIN - Duration::from_secs(1);
        assert!(token_needs_refresh(expires_at));
    }

    #[test]
    fn token_needs_refresh_when_exactly_at_margin_boundary() {
        let expires_at = SystemTime::now() + REFRESH_MARGIN;
        assert!(token_needs_refresh(expires_at));
    }

    #[test]
    fn token_does_not_need_refresh_when_safely_outside_margin() {
        let expires_at = SystemTime::now() + REFRESH_MARGIN + Duration::from_secs(1);
        assert!(!token_needs_refresh(expires_at));
    }
}
