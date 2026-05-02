use crate::config::ConnectorConfig;
use crate::firebase::FirebaseConnector;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::watch;

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn send_event(&self, event_type: &str, payload: &Value);
}

pub type SinkSender = watch::Sender<Arc<dyn EventSink + Send + Sync>>;
pub type SinkReceiver = watch::Receiver<Arc<dyn EventSink + Send + Sync>>;

#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

#[async_trait]
impl EventSink for NullSink {
    async fn send_event(&self, _event_type: &str, _payload: &Value) {}
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
