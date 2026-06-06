use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct MatchIndexEntry {
    pub timestamp: u64,
    pub blue_score: u64,
    pub orange_score: u64,
    pub match_id: String,
    pub blue_team_id: Option<String>,
    pub blue_shots: u64,
    pub blue_saves: u64,
    pub blue_assists: u64,
    pub blue_demos: u64,
    pub orange_team_id: Option<String>,
    pub orange_shots: u64,
    pub orange_saves: u64,
    pub orange_assists: u64,
    pub orange_demos: u64,
}

#[derive(Debug, Serialize)]
pub struct PlayerMatchLog {
    pub timestamp: u64,
    pub goals: u64,
    pub shots: u64,
    pub saves: u64,
    pub assists: u64,
    pub score: i64,
    pub touches: u64,
    pub demos: u64,
    pub match_id: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CumulativePlayerStats {
    pub goals: u64,
    pub assists: u64,
    pub saves: u64,
    pub shots: u64,
    pub wins: u64,
    pub losses: u64,
    pub mvps: u64,
    pub score: i64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct PlayerRegistryEntry {
    pub team_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CumulativeTeamStats {
    pub matches_played: u64,
    pub wins: u64,
    pub losses: u64,
    pub goals_for: u64,
    pub goals_against: u64,
    pub shots: u64,
    pub saves: u64,
    pub assists: u64,
    pub demos: u64,
}
