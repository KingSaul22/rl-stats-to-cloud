pub mod connector;
pub mod config;
#[path = "daemon/mod.rs"]
pub mod daemon;
pub mod firebase;
#[path = "worker/mod.rs"]
pub mod worker;

pub use connector::{connector_factory, EventSink, SinkReceiver, SinkSender};
pub use config::{default_config_path, AppConfig, ConfigManager};
pub use firebase::FirebaseConnector;
pub use worker::RocketLeagueWorker;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub is_connected: bool,
    pub last_event: String,
}

pub type StateSender = tokio::sync::watch::Sender<AppState>;
pub type StateReceiver = tokio::sync::watch::Receiver<AppState>;
