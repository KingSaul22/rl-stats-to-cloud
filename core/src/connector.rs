use crate::config::ConnectorConfig;
use crate::firebase::FirebaseConnector;
use crate::firebase_auth::FirebaseAuth;
use async_trait::async_trait;
use serde_json::Value;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkError {
    TransientNetwork { message: String },
    RateLimited { message: String },
    Terminal { message: String },
}

impl SinkError {
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self::TransientNetwork {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::RateLimited {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn terminal(message: impl Into<String>) -> Self {
        Self::Terminal {
            message: message.into(),
        }
    }
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransientNetwork { message } => {
                write!(f, "transient sink error: {message}")
            }
            Self::RateLimited { message } => {
                write!(f, "rate-limited sink error: {message}")
            }
            Self::Terminal { message } => {
                write!(f, "terminal sink error: {message}")
            }
        }
    }
}

impl Error for SinkError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkLane {
    LiveState,
    EventFeed,
    Historical,
}

impl SinkLane {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LiveState => "live_state",
            Self::EventFeed => "event_feed",
            Self::Historical => "historical",
        }
    }
}

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn send_event(&self, event_type: &str, payload: &Value) -> Result<(), SinkError>;

    async fn send_event_on_lane(
        &self,
        _lane: SinkLane,
        event_type: &str,
        payload: &Value,
    ) -> Result<(), SinkError> {
        self.send_event(event_type, payload).await
    }

    /// Deletes a connector node when supported by the backend sink.
    ///
    /// # Errors
    /// Returns an error when the sink cannot perform deletes or when the
    /// underlying connector request fails.
    async fn delete_node(&self, path: &str) -> Result<(), SinkError> {
        Err(SinkError::terminal(format!(
            "sink does not support delete_node for path '{path}'"
        )))
    }

    /// Puts a JSON node to a connector path when supported by the backend sink.
    ///
    /// # Errors
    /// Returns an error when the sink cannot perform puts or when the
    /// underlying connector request fails.
    async fn put_node(&self, path: &str, _data: &Value) -> Result<(), SinkError> {
        Err(SinkError::terminal(format!(
            "sink does not support put_node for path '{path}'"
        )))
    }

    /// Retrieves a JSON node from the backend sink.
    /// Returns `Ok(None)` when the node does not exist (null response).
    ///
    /// # Errors
    /// Returns an error when the request fails or the backend returns
    /// a non-success status code.
    async fn get_node(&self, path: &str) -> Result<Option<Value>, SinkError> {
        Err(SinkError::terminal(format!(
            "sink does not support get_node for path '{path}'"
        )))
    }
}
pub use EventSink as TelemetrySink;

pub type SinkSender = watch::Sender<Arc<dyn EventSink + Send + Sync>>;
pub type SinkReceiver = watch::Receiver<Arc<dyn EventSink + Send + Sync>>;

#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

#[async_trait]
impl EventSink for NullSink {
    async fn send_event(&self, _event_type: &str, _payload: &Value) -> Result<(), SinkError> {
        Ok(())
    }

    async fn send_event_on_lane(
        &self,
        _lane: SinkLane,
        _event_type: &str,
        _payload: &Value,
    ) -> Result<(), SinkError> {
        Ok(())
    }

    async fn delete_node(&self, _path: &str) -> Result<(), SinkError> {
        Ok(())
    }

    async fn put_node(&self, _path: &str, _data: &Value) -> Result<(), SinkError> {
        Ok(())
    }

    async fn get_node(&self, _path: &str) -> Result<Option<Value>, SinkError> {
        Ok(None)
    }
}

#[must_use]
pub fn connector_factory(config: &ConnectorConfig) -> Arc<dyn EventSink + Send + Sync> {
    let (sink, _auth) = connector_factory_with_auth(config);
    sink
}

#[must_use]
pub fn connector_factory_with_auth(
    config: &ConnectorConfig,
) -> (Arc<dyn EventSink + Send + Sync>, Option<FirebaseAuth>) {
    match config {
        ConnectorConfig::Firebase {
            url,
            api_key,
            email,
            password,
        } => {
            let auth = FirebaseAuth::new(api_key.clone(), email.clone(), password.clone());
            let connector = FirebaseConnector::new(url.clone(), auth.clone());
            (Arc::new(connector), Some(auth))
        }
    }
}
