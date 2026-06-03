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

const LOGIN_ENDPOINT: &str = "https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword";
const REFRESH_ENDPOINT: &str = "https://securetoken.googleapis.com/v1/token";
const REFRESH_MARGIN: Duration = Duration::from_mins(5);

#[derive(Debug, Clone)]
pub struct TokenState {
    pub id_token: String,
    pub refresh_token: String,
    pub expires_at: SystemTime,
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
    password: String,
    http: Client,
    state: Arc<RwLock<TokenState>>,
}

impl FirebaseAuth {
    #[must_use]
    pub fn new(api_key: String, email: String, password: String) -> Self {
        let http = match Client::builder().timeout(Duration::from_secs(5)).build() {
            Ok(client) => client,
            Err(err) => {
                eprintln!(
                    "Firebase auth warning: failed to build timed HTTP client ({err}). Falling back to default reqwest client."
                );
                Client::new()
            }
        };

        Self {
            api_key,
            email,
            password,
            http,
            state: Arc::new(RwLock::new(TokenState::default())),
        }
    }

    /// Authenticate with Firebase Identity Toolkit and cache ID/refresh tokens.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails, Firebase rejects credentials, or response payload is invalid.
    pub async fn login(&self) -> Result<(), AuthError> {
        let url = format!("{LOGIN_ENDPOINT}?key={}", self.api_key);
        let payload = SignInRequest {
            email: self.email.clone(),
            password: self.password.clone(),
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
            return Err(AuthError::MissingTokenState);
        }

        Ok(state.id_token.clone())
    }

    async fn refresh_token(&self) -> Result<(), AuthError> {
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
        Ok(())
    }
}

#[derive(Debug)]
pub enum AuthError {
    Request(reqwest::Error),
    HttpStatus { status: StatusCode, body: String },
    ParseExpiresIn(std::num::ParseIntError),
    MissingTokenState,
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
        }
    }
}

impl Error for AuthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Request(err) => Some(err),
            Self::ParseExpiresIn(err) => Some(err),
            Self::HttpStatus { .. } | Self::MissingTokenState => None,
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