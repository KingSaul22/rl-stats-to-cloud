# ADR 0023: Denormalised Team Stats in `matches_index`

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0023 |
| **Title** | Denormalised Team Stats in `matches_index` |
| **Status** | Accepted |
| **Date** | 2026-06-06 |

## Context
The Android (Kotlin) client must render team progression charts — per-match trends for shots, saves, assists, and demos across a season. In a relational database, this is a straightforward join between a `matches` table and a `team_stats` table. In Firebase RTDB, a NoSQL document store with no server-side join capability, there are two canonical approaches: create a dedicated `team_match_logs` node, or denormalise the data into an existing node that the client already queries.

ADR 0021 established `matches_index/{match_id}` as the lightweight summary node for match results (scores, timestamps). ADR 0022 established the majority-rule roster resolution that identifies which registered team occupied each side of a match. These two decisions created the necessary inputs — the router can now resolve `blue_team_id` and `orange_team_id` during aggregation, and `matches_index` already serves as the canonical match-level lookup node.

A decision was required on where to store per-team-per-match statistical aggregates for client-side progression queries.

## Decision
We implement a **Dual-Purpose Index Enrichment** pattern. Rather than creating a separate `team_match_logs` node, we enrich the existing `matches_index` entry with the resolved team identities and their per-match statistical aggregates. The Kotlin client queries a single RTDB path with a single index:

```
matches_index?orderBy="blue_team_id"&equalTo="eclipse_total"
```

### Schema Expansion
Ten fields are added to `MatchIndexEntry` — five per side:

| Field | Type | Source |
|-------|------|--------|
| `blue_team_id` | `Option<String>` | `resolve_team_id` majority-rule output (ADR 0022) |
| `blue_shots` | `u64` | Sum of all blue-side player `shots` fields from `master_state` player telemetry |
| `blue_saves` | `u64` | Sum of all blue-side player `saves` fields |
| `blue_assists` | `u64` | Sum of all blue-side player `assists` fields |
| `blue_demos` | `u64` | Sum of all blue-side player `demos` fields |
| `orange_team_id` | `Option<String>` | `resolve_team_id` majority-rule output (ADR 0022) |
| `orange_shots` | `u64` | Sum of all orange-side player stats |
| `orange_saves` | `u64` | —"— |
| `orange_assists` | `u64` | —"— |
| `orange_demos` | `u64` | —"— |

Empty teams (no players on a side) receive `0` for all stat fields and `null` for `team_id`.

### Summation Hoisting
The per-player stat summation was previously performed inside `update_cumulative_team_stats` — the function that persists cumulative team stats to `stats_cumulative_teams/{team_id}`. The summation loop is hoisted up into `upload_aggregation`, the orchestrator function that already has access to the per-team player data vectors (`blue_players`, `orange_players`). A local `sum_team_stats` helper extracts shots, saves, assists, and demos from each slice of `&Value` references using the existing `extract_u64` utility.

The summation is performed **once**, and the results are fed to both consumers:
1. The `MatchIndexEntry` struct — for the enriched `matches_index` JSON
2. `update_cumulative_team_stats` — as four pre-computed `u64` parameters, replacing the removed `&[&Value]` slice parameter

```
┌──────────────────────────────┐
│ group players by team 0/1    │
│ (blue_players, orange_players)│
└─────────────┬────────────────┘
              │
              ▼
┌──────────────────────────────┐
│ resolve_team_id ×2           │  (ADR 0022)
│ (concurrent join)            │
└─────────────┬────────────────┘
              │
              ▼
┌──────────────────────────────┐
│ sum_team_stats ×2             │
│ → blue_shots, saves, etc.    │
│ → orange_shots, saves, etc.  │
└──────┬───────────┬───────────┘
       │           │
       ▼           ▼
┌─────────────┐ ┌─────────────────────┐
│ index_entry │ │ update_cumulative   │
│ (enriched)  │ │ _team_stats(pre-    │
│             │ │ computed sums)      │
│ PUTs to     │ │ GET→MODIFY→PUT to   │
│ matches_    │ │ stats_cumulative_   │
│ index/{id}  │ │ teams/{team_id}     │
└─────────────┘ └─────────────────────┘
```

### Query Patterns Enabled
The enriched `matches_index` node supports the following Kotlin client queries without secondary nodes:

| Use case | RTDB query |
|----------|------------|
| Team match history | `?orderBy="blue_team_id"&equalTo="eclipse_total"` |
| Team progression chart | Iterate results, extract `blue_shots` per entry, plot over `timestamp` |
| Both sides | Run the same query with `orange_team_id` |
| Recent global matches | `?orderBy="timestamp"&limitToLast=20` (plus extra bytes per entry) |
| Head-to-head | Filter in Kotlin: `blue_team_id==A && orange_team_id==B \|\| blue_team_id==B && orange_team_id==A` |

## Rejected Alternatives
- **Dedicated `team_match_logs/{team_id}/{match_id}` node:** Rejected because it duplicates every statistical value already computed in `matches_index`. A typical 3v3 match would have 6 redundant stat fields split across two nodes. At scale across hundreds of matches and dozens of teams, this bloat is measurable in Firebase storage and bandwidth billing.
- **Client-side aggregation from `player_match_logs`:** Rejected because it requires the mobile client to download full player-level data for every team member across every match, then sum and group client-side. This is the "Fat Read" anti-pattern ADR 0021 was designed to eliminate. It also requires the client to know which players belong to which team, coupling the mobile app to roster data.
- **Separate `team_progression` node with pre-aggregated time-series data:** Rejected because it shifts the aggregation surface from the daemon to Firebase, requiring a Cloud Function or an additional write-path in the daemon that mirrors team stats to yet another node. This adds infrastructure complexity with no corresponding benefit over the enriched `matches_index` approach.
- **Firebase RTDB Rules-based computed fields:** Rejected because RTDB has no server-side computation capability. Computed fields are not supported.

## Consequences

### Positive
- **Single-node team progression queries:** The Kotlin client retrieves a team's entire season history — scores, per-match stats, timestamps — with one RTDB query. No client-side joins, no secondary fetches, no data duplication.
- **Zero storage bloat:** The `matches_index/{match_id}` node grows by approximately 10 fields per entry (~150 bytes). No additional nodes are created. A `team_match_logs` node would double this for the same data.
- **Reduced Firebase bandwidth and operation count:** A team-progression query that previously required fetching `matches_index` (for scores) plus a hypothetical `team_match_logs` (for stats) is now a single read operation. For a season of 50 matches, this saves 50 read operations per team per client session.
- **Computation at ingestion time:** The stat summation runs in the Rust daemon at match conclusion using in-memory data. No additional network round-trips. The summation is computed once and hydrates both the enriched index and the cumulative team stats simultaneously.
- **Consistent identity source:** `blue_team_id` and `orange_team_id` come from the same majority-rule resolution (ADR 0022) that feeds `stats_cumulative_teams`. The Kotlin client sees the same team identity in both nodes, avoiding reconciliation bugs.

### Negative / Limitations
- **Slightly larger `matches_index` payloads:** A global "recent matches" query (`orderBy="timestamp"&limitToLast=20`) now downloads 10 additional fields per match. For 20 matches, this is approximately 3 KB of extra data. This is a favourable trade-off: the data is useful for match-result display and would otherwise require a secondary query.
- **Tight coupling between `matches_index` schema and the summation logic:** If a new player stat is added to the telemetry (e.g., `boost_used`), the `MatchIndexEntry` struct, the `sum_team_stats` helper, and `update_cumulative_team_stats` all need coordinated updates. This coupling is inherent to denormalisation and is acceptable given the low frequency of schema changes.
- **Empty teams produce zero-valued fields:** If a match side has no players (unlikely in practice, but possible during edge-case transitions), the stat fields are `0` and `team_id` is `null`. The Kotlin client must handle `null` gracefully when rendering team-specific views.

### Mitigations
- The `sum_team_stats` helper encapsulates the summation logic in one place. Adding a new stat requires only: (a) adding the field to `MatchIndexEntry`, (b) adding one loop line in `sum_team_stats`, and (c) adding the corresponding accumulator in `update_cumulative_team_stats`.
- The `0`-valued fields for empty teams are semantically correct (no players → no stats) and indistinguishable from a real team that scored zero in a category.
