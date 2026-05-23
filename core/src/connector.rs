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

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn send_event(&self, event_type: &str, payload: &Value) -> Result<(), SinkError>;
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
}

#[must_use]
pub fn connector_factory(config: &ConnectorConfig) -> Arc<dyn EventSink + Send + Sync> {
    match config {
        ConnectorConfig::Firebase { url, auth_token } => Arc::new(FirebaseConnector::new(
            url.clone(),
            auth_token.clone(),
        )),
    }
}
