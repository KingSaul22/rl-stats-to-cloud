# ADR 0022: Database-Driven Majority-Rule Roster Resolution

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0022 |
| **Title** | Database-Driven Majority-Rule Roster Resolution |
| **Status** | Accepted |
| **Date** | 2026-06-05 |

## Context
ADR 0021 established edge data aggregation for player-level cumulative stats. The natural extension is team-level cumulative stats — tracking wins, losses, goals, and aggregate player performance per registered team across all matches. However, a team participating in a Rocket League match on the Blue or Orange side cannot be identified from in-game telemetry alone. The game engine only exposes a lobby name string (e.g., `"NRG"` or `"NRG Esports"`), which is brittle: a single typo, trailing space, or abbreviation variant would fork team stats into two separate leaderboard entries, corrupting cumulative records.

Furthermore, eSports rosters include substitute players. A match may feature 2 core members of Team Alpha plus 1 substitute who is registered to the same team. The resolution system must recognise that 2 of 3 players share a `team_id` and correctly attribute the match to Team Alpha, even though the third player's registration provides a confirming vote.

A decision was required to establish a reliable, database-driven identity resolution mechanism that is immune to client-side naming inconsistencies.

## Decision
We implement **Database-Driven Majority-Rule Roster Resolution**. When the aggregation pipeline processes a match, it resolves which registered team occupied the Blue side and which occupied the Orange side by querying the Firebase player registry.

### Resolution Algorithm

For each in-match team (Blue = team 0, Orange = team 1):

1. **Extract player identities:** Iterate `player_telemetry` from the `live_state` snapshot. Collect the sanitised Epic/Steam IDs of all players assigned to that match team.

2. **Concurrent registry lookups:** For each sanitised player ID, issue a `GET /players/{id}` request to the Firebase RTDB. All lookups run concurrently via `futures_util::future::join_all` to minimise latency. Each response is deserialised into a `PlayerRegistryEntry` containing an optional `team_id` field.

3. **Tally frequencies:** Count how many players on the match team are registered to each `team_id`. Players whose registry lookup fails, returns no entry, or has no `team_id` are excluded from the tally (they cast no vote).

4. **Apply strict majority rule:** The resolution returns the `team_id` with the highest count if and only if its count is strictly greater than every *other* `team_id`'s count and greater than zero:

| Players on team | Registrations | Majority? | Result |
|:---|:---|:---:|:---|
| 3 | 3 × `"EG"` | Yes | `"EG"` |
| 3 | 2 × `"EG"`, 1 × `"NRG"` | Yes (2 > 1) | `"EG"` |
| 3 | 2 × `"EG"`, 1 unregistered | Yes (2 > 0) | `"EG"` |
| 4 | 2 × `"EG"`, 2 × `"NRG"` | No (2 ≯ 2) | Fallback |
| 2 | 1 × `"EG"`, 1 × `"NRG"` | No (1 ≯ 1) | Fallback |
| 3 | All unregistered | No (no votes) | Fallback |

### Fallback: Deterministic Temporary IDs
When no majority exists, the system does not discard data. Instead it generates a deterministic fallback ID: `temp_{match_id}_blue` or `temp_{match_id}_orange`. These paths isolate the match's stats without misattributing them to any registered team. A future reconciliation pass (or manual admin action) can migrate these temp entries once the correct team identity is established.

### Integration into the Aggregation Pipeline
Resolution runs in the `upload_aggregation` function, after player stat extraction and before the main `join_all` of upload futures. Both teams' resolutions execute concurrently via `futures_util::future::join`. The resolved team IDs are then passed to `update_cumulative_team_stats`, which performs the standard GET-MODIFY-PUT cycle against `stats_cumulative_teams/{team_id}` with three-retry full-jitter exponential backoff.

```
┌────────────────────────┐
│ Group players by        │
│ extract_team (0/1)      │
└───────────┬────────────┘
            │
    ┌───────┴───────┐
    ▼               ▼
┌───────────┐  ┌───────────┐
│ Resolve   │  │ Resolve   │
│ Blue team │  │ Orange    │
│ (join_all │  │ team      │
│  of GETs) │  │ (join_all │
└─────┬─────┘  │  of GETs) │
      │        └─────┬─────┘
      │              │
      └──────┬───────┘
             │ (futures_util::future::join)
             ▼
┌────────────────────────┐
│ update_cumulative       │
│ _team_stats ×2 (R-M-W)  │
└────────────────────────┘
```

## Rejected Alternatives
- **Trust the client-provided lobby name string:** Rejected because Rocket League lobby names are free-form text entered by players. A single character difference (`"NRG"` vs `"NRG "`) would create a separate stats entry, permanently fragmenting a team's historical record. No amount of server-side normalisation can reliably resolve arbitrary string variance.
- **Assign team identity via a hardcoded player-to-team mapping in the daemon config:** Rejected because it requires the daemon operator to maintain a manually curated roster file. This adds operational toil, drifts out of sync as rosters change, and cannot scale to multiple concurrent leagues or ad-hoc tournament matches.
- **Use a single player's registration as the authoritative team ID:** Rejected because it provides no defence against substitute players. If the "authoritative" player is substituted out for a match, the entire team stat attribution breaks. The majority rule naturally handles substitute scenarios without configuration.
- **Require all players on a match team to share the same `team_id`:** Rejected because this rejects valid matches where a substitute from a different registered team or an unregistered free agent fills a slot. Strict unanimity would cause unnecessary fallbacks and data isolation.
- **Run team resolution on the client (mobile app) post-hoc:** Rejected because it creates a dependency between stat accuracy and client uptime. The mobile app may be offline when a match concludes. The daemon is the single authority on match lifecycle and must resolve identity at ingestion time.

## Consequences

### Positive
- **Immune to client-side naming errors:** The resolution logic never inspects the lobby name string. Identity is derived exclusively from the Firebase player registry, which is curated by team administrators and serves as the single source of truth.
- **Native substitute support:** The strict majority rule naturally accommodates substitute players. As long as a majority of players on a match team share a registered `team_id`, the resolution is correct regardless of who the remaining players are registered to.
- **No data loss under ambiguity:** The deterministic temp-ID fallback ensures that every match contributes to some stat entry, even when team identity cannot be resolved. Temp entries can be migrated later via a reconciliation pass.
- **Concurrent resolution minimises latency:** Both teams' registry lookups run concurrently within each team (via `join_all`), and both teams resolve concurrently (via `futures_util::future::join`). Worst-case latency is bounded by the slowest of up to 6 parallel GET requests.
- **Single source of truth:** The `team_id` is a stable database key. If a team rebrands or changes its display name, the stats follow the key without requiring migration.

### Negative / Limitations
- **HTTP GET burst during aggregation:** For a 6-player match (3v3), the system issues up to 6 concurrent GET requests to resolve both teams, in addition to the existing PUT requests for match index, player logs, and cumulative stats. This briefly increases network I/O at the moment of match transition.
- **Registry dependency:** If the `/players/{id}` nodes are unavailable during aggregation (Firebase outage, rate-limiting), all players are treated as unregistered and the fallback temp ID is used. Consecutive matches under these conditions would each create isolated temp entries, fragmenting what would otherwise be cumulative team stats.
- **Cold-start for new players:** A player who has not yet been registered in the database casts no vote during resolution. Teams with a majority of unregistered players will always fall back to temp IDs until the registry is populated.
- **No reconciliation pass exists:** Temp IDs are never automatically migrated to real team IDs. The `temp_{match_id}_{team}` entries accumulate indefinitely until a future administrative tool or ADR addresses reconciliation.

### Mitigations
- Temp IDs embed the `match_id`, making them traceable back to the exact match. This supports future reconciliation tooling.
- The `resolve_team_id` function is fail-soft: failed GET requests simply exclude that player from the tally rather than aborting the entire resolution.
- The concurrent GET pattern via `join_all` runs the lookups in parallel, keeping the latency cost linear in the number of players rather than multiplicative.
