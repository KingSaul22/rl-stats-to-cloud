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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn match_index_entry_serializes_complete_wire_shape() {
        let entry = MatchIndexEntry {
            timestamp: 1_715_001_200,
            blue_score: 3,
            orange_score: 2,
            match_id: "match_abc".to_string(),
            blue_team_id: Some("eclipse_total".to_string()),
            blue_shots: 8,
            blue_saves: 5,
            blue_assists: 3,
            blue_demos: 2,
            orange_team_id: Some("nrg_esports".to_string()),
            orange_shots: 6,
            orange_saves: 3,
            orange_assists: 1,
            orange_demos: 3,
        };

        let v = serde_json::to_value(&entry).unwrap_or_default();

        assert_eq!(v["timestamp"], json!(1_715_001_200_u64));
        assert_eq!(v["blue_score"], json!(3_u64));
        assert_eq!(v["orange_score"], json!(2_u64));
        assert_eq!(v["match_id"], json!("match_abc"));
        assert_eq!(v["blue_team_id"], json!("eclipse_total"));
        assert_eq!(v["blue_shots"], json!(8_u64));
        assert_eq!(v["blue_saves"], json!(5_u64));
        assert_eq!(v["blue_assists"], json!(3_u64));
        assert_eq!(v["blue_demos"], json!(2_u64));
        assert_eq!(v["orange_team_id"], json!("nrg_esports"));
        assert_eq!(v["orange_shots"], json!(6_u64));
        assert_eq!(v["orange_saves"], json!(3_u64));
        assert_eq!(v["orange_assists"], json!(1_u64));
        assert_eq!(v["orange_demos"], json!(3_u64));
    }

    #[test]
    fn match_index_entry_serializes_absent_team_ids_as_null() {
        let entry = MatchIndexEntry {
            timestamp: 0,
            blue_score: 0,
            orange_score: 0,
            match_id: "match_null".to_string(),
            blue_team_id: None,
            blue_shots: 0,
            blue_saves: 0,
            blue_assists: 0,
            blue_demos: 0,
            orange_team_id: None,
            orange_shots: 0,
            orange_saves: 0,
            orange_assists: 0,
            orange_demos: 0,
        };

        let v = serde_json::to_value(&entry).unwrap_or_default();

        assert_eq!(v["blue_team_id"], json!(null));
        assert_eq!(v["orange_team_id"], json!(null));
    }

    #[test]
    fn cumulative_team_stats_serializes_all_numeric_fields() {
        let stats = CumulativeTeamStats {
            matches_played: 10,
            wins: 7,
            losses: 3,
            goals_for: 25,
            goals_against: 14,
            shots: 52,
            saves: 30,
            assists: 18,
            demos: 12,
        };

        let v = serde_json::to_value(&stats).unwrap_or_default();

        assert_eq!(v["matches_played"], json!(10_u64));
        assert_eq!(v["wins"], json!(7_u64));
        assert_eq!(v["losses"], json!(3_u64));
        assert_eq!(v["goals_for"], json!(25_u64));
        assert_eq!(v["goals_against"], json!(14_u64));
        assert_eq!(v["shots"], json!(52_u64));
        assert_eq!(v["saves"], json!(30_u64));
        assert_eq!(v["assists"], json!(18_u64));
        assert_eq!(v["demos"], json!(12_u64));
    }

    #[test]
    fn cumulative_team_stats_deserializes_missing_fields_to_zero() {
        let json = json!({"wins": 5, "goals_for": 12});
        let stats: CumulativeTeamStats = serde_json::from_value(json).unwrap_or_default();

        assert_eq!(stats.wins, 5);
        assert_eq!(stats.goals_for, 12);
        assert_eq!(stats.matches_played, 0);
        assert_eq!(stats.losses, 0);
        assert_eq!(stats.goals_against, 0);
        assert_eq!(stats.shots, 0);
        assert_eq!(stats.saves, 0);
        assert_eq!(stats.assists, 0);
        assert_eq!(stats.demos, 0);
    }

    #[test]
    fn cumulative_team_stats_rejects_null_numeric_fields() {
        let json = json!({"wins": null, "goals_for": 5});
        let result = serde_json::from_value::<CumulativeTeamStats>(json);

        assert!(result.is_err());
    }
}
