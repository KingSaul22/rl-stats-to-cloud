pub mod config;
pub mod connector;
#[path = "daemon/mod.rs"]
pub mod daemon;
pub mod firebase;
pub mod firebase_auth;
pub mod models;
#[path = "worker/mod.rs"]
pub mod worker;

pub use config::{AppConfig, ConfigManager, default_config_path};
pub use connector::{EventSink, SinkReceiver, SinkSender, connector_factory};
pub use firebase::FirebaseConnector;
pub use firebase_auth::{AuthError, AuthRuntimeState, FirebaseAuth, TokenState};
use serde::{Deserialize, Serialize};
pub use worker::RocketLeagueWorker;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub is_connected: bool,
    pub last_event: String,
}

pub type StateSender = tokio::sync::watch::Sender<AppState>;
pub type StateReceiver = tokio::sync::watch::Receiver<AppState>;
