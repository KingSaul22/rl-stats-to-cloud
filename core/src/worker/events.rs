use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestClass {
    LiveState,
    EventFeed,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RocketLeagueEvent {
    UpdateState,
    ClockUpdated,
    Goal,
    Save,
    Demolition,
    EventFeedMarker,
    MatchHistoryMarker,
    Unknown(String),
}

impl RocketLeagueEvent {
    #[must_use]
    pub(crate) fn from_event_name(event_name: String) -> Self {
        match event_name.as_str() {
            "UpdateState" => Self::UpdateState,
            "ClockUpdated" | "ClockUpdatedSeconds" => Self::ClockUpdated,
            "Goal" | "GoalScored" => Self::Goal,
            "Save" | "EpicSave" => Self::Save,
            "Demolition" | "Demo" => Self::Demolition,
            "EventFeedMarker" => Self::EventFeedMarker,
            "MatchHistoryMarker" => Self::MatchHistoryMarker,
            _ => Self::Unknown(event_name),
        }
    }
}

impl<'de> Deserialize<'de> for RocketLeagueEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let event_name = String::deserialize(deserializer)?;
        Ok(Self::from_event_name(event_name))
    }
}

#[derive(Debug, Clone)]
pub struct IngestEnvelope {
    pub(crate) seq: u64,
    pub(crate) event_type: String,
    pub(crate) payload: Value,
    pub(crate) class: IngestClass,
    pub(crate) active_match_id: String,
}

impl IngestEnvelope {
    #[must_use]
    pub(crate) fn bootstrap() -> Self {
        Self {
            seq: 0,
            event_type: "__bootstrap__".to_string(),
            payload: Value::Null,
            class: IngestClass::LiveState,
            active_match_id: String::new(),
        }
    }
}
