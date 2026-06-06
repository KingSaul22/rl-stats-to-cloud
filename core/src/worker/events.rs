use serde::Deserialize;
use serde_json::Value;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestClass {
    LiveState,
    EventFeed,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RocketLeagueEvent {
    UpdateState,
    BallHit,
    ClockUpdatedSeconds,
    CountdownBegin,
    CrossbarHit,
    GoalReplayEnd,
    GoalReplayStart,
    GoalReplayWillEnd,
    GoalScored,
    MatchCreated,
    MatchInitialized,
    MatchDestroyed,
    MatchEnded,
    MatchPaused,
    MatchUnpaused,
    PodiumStart,
    ReplayCreated,
    RoundStarted,
    StatfeedEvent,
    Unknown(String),
}

impl RocketLeagueEvent {
    #[must_use]
    pub(crate) fn from_event_name(event_name: String) -> Self {
        match event_name.as_str() {
            "UpdateState" => Self::UpdateState,
            "BallHit" => Self::BallHit,
            // Soportamos tanto el nombre oficial como el alias corto de reloj
            "ClockUpdatedSeconds" | "ClockUpdated" => Self::ClockUpdatedSeconds,
            "CountdownBegin" => Self::CountdownBegin,
            "CrossbarHit" => Self::CrossbarHit,
            "GoalReplayStart" => Self::GoalReplayStart,
            // Mapeamos el fin de la repetición de forma segura
            "GoalReplayEnd" => Self::GoalReplayEnd,
            "GoalReplayWillEnd" => Self::GoalReplayWillEnd,
            // Soportamos tanto el nombre oficial como el alias corto de gol
            "GoalScored" | "Goal" => Self::GoalScored,
            "MatchCreated" => Self::MatchCreated,
            "MatchInitialized" => Self::MatchInitialized,
            "MatchDestroyed" => Self::MatchDestroyed,
            "MatchEnded" => Self::MatchEnded,
            "MatchPaused" => Self::MatchPaused,
            "MatchUnpaused" => Self::MatchUnpaused,
            "PodiumStart" => Self::PodiumStart,
            "ReplayCreated" => Self::ReplayCreated,
            "RoundStarted" => Self::RoundStarted,
            "StatfeedEvent" => Self::StatfeedEvent,
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

#[derive(Debug)]
pub enum TransientLaneMessage {
    Event(IngestEnvelope),
    Flush { ack: oneshot::Sender<()> },
    Snapshot { result: oneshot::Sender<Value> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_event_name_maps_goal_scored() {
        assert_eq!(
            RocketLeagueEvent::from_event_name("GoalScored".to_string()),
            RocketLeagueEvent::GoalScored
        );
    }

    #[test]
    fn from_event_name_maps_replay_events() {
        assert_eq!(
            RocketLeagueEvent::from_event_name("GoalReplayStart".to_string()),
            RocketLeagueEvent::GoalReplayStart
        );
        assert_eq!(
            RocketLeagueEvent::from_event_name("GoalReplayEnd".to_string()),
            RocketLeagueEvent::GoalReplayEnd
        );
        assert_eq!(
            RocketLeagueEvent::from_event_name("GoalReplayWillEnd".to_string()),
            RocketLeagueEvent::GoalReplayWillEnd
        );
    }
}
