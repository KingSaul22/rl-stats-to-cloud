use crate::config::ConnectorConfig;
use crate::firebase::FirebaseConnector;
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
}

#[must_use]
pub async fn connector_factory(config: &ConnectorConfig) -> Arc<dyn EventSink + Send + Sync> {
    match config {
        ConnectorConfig::Firebase {
            url,
            api_key,
            email,
            password,
        } => match FirebaseConnector::new(
            url.clone(),
            api_key.clone(),
            email.clone(),
            password.clone(),
        )
        .await
        {
            Ok(connector) => Arc::new(connector),
            Err(err) => {
                eprintln!(
                    "Firebase connector warning: failed to initialize auth session ({err}). Falling back to null sink."
                );
                Arc::new(NullSink)
            }
        },
    }
}
