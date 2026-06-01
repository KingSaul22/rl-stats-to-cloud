# UpdateState

**Category:** Tick

## Description
Sent X amount of times per second based on the player's PacketSendRate preference.

## Example Payload
```json
{
  "Event": "UpdateState",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6",
    "Players": [
      {
        "Name": "PlayerA",
        "PrimaryId": "Steam|123|0",
        "Shortcut": 1,
        "TeamNum": 0,
        "Score": 125,
        "Goals": 1,
        "Shots": 2,
        "Assists": 0,
        "Saves": 1,
        "Touches": 14,
        "CarTouches": 3,
        "Demos": 0,
        "bHasCar": true,
        "Speed": 1200,
        "Boost": 45,
        "bBoosting": true,
        "bOnGround": true,
        "bOnWall": false,
        "bPowersliding": false,
        "bDemolished": true,
        "Attacker": {
          "Name": "PlayerB",
          "Shortcut": 2,
          "TeamNum": 1
        },
        "bSupersonic": true
      }
    ],
    "Game": {
      "Teams": [
        {
          "Name": "Blue",
          "TeamNum": 0,
          "Score": 1,
          "ColorPrimary": "0000FF",
          "ColorSecondary": "0000AA"
        }
      ],
      "TimeSeconds": 180,
      "bOvertime": false,
      "Frame": 120,
      "Elapsed": 50.2,
      "Ball": {
        "Speed": 850.5,
        "TeamNum": 0
      },
      "bReplay": false,
      "bHasWinner": true,
      "Winner": "Blue",
      "Arena": "Stadium_P",
      "bHasTarget": true,
      "Target": {
        "Name": "PlayerA",
        "Shortcut": 1,
        "TeamNum": 0
      }
    }
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `Players` | `array` | One entry per player in the match. |
| `Players.Name` | `string` | Display name. |
| `Players.PrimaryId` | `string` | Platform identifier in the format `Platform|Uid|Splitscreen` (e.g. `Steam|123|0`, `Epic|456|0`). |
| `Players.Shortcut` | `int` | Spectator shortcut number. |
| `Players.TeamNum` | `int` | Team index (`0 = Blue`, `1 = Orange`). |
| `Players.Score` | `int` | Total match score. |
| `Players.Goals` | `int` | Goals scored this match. |
| `Players.Shots` | `int` | Shot attempts this match. |
| `Players.Assists` | `int` | Assists earned this match. |
| `Players.Saves` | `int` | Saves made this match. |
| `Players.Touches` | `int` | Total ball touches. |
| `Players.CarTouches` | `int` | Touches by the car body (not ball). |
| `Players.Demos` | `int` | Demolitions inflicted. |
| `Players.bHasCar` | `bool` | SPECTATOR only. True if the player currently has a vehicle. |
| `Players.Speed` | `float` | SPECTATOR only. Vehicle speed in Unreal Units/second. |
| `Players.Boost` | `int` | SPECTATOR only. Boost amount 0-100. |
| `Players.bBoosting` | `bool` | SPECTATOR only. True if the player is currently boosting. |
| `Players.bOnGround` | `bool` | SPECTATOR only. True if at least 3 wheels are touching the world. |
| `Players.bOnWall` | `bool` | SPECTATOR only. True if the vehicle is on a wall. |
| `Players.bPowersliding` | `bool` | SPECTATOR only. True if the player is holding handbrake. |
| `Players.bDemolished` | `bool` | SPECTATOR only. True if the vehicle is currently destroyed. |
| `Players.bSupersonic` | `bool` | SPECTATOR only. True if the vehicle is at supersonic speed. |
| `Players.Attacker` | `object` | CONDITIONAL. The player who demolished this player. Present only when demolished. |
| `Players.Attacker.Name` | `string` | Name of the player who demolished this player. |
| `Players.Attacker.Shortcut` | `int` | Spectator shortcut of the attacker. |
| `Players.Attacker.TeamNum` | `int` | Team index of the attacker. |
| `Game` | `object` | Match metadata. |
| `Game.Teams` | `array` | One entry per team, ordered by `TeamNum`. |
| `Game.Teams.Name` | `string` | Team name. |
| `Game.Teams.TeamNum` | `int` | Team index. |
| `Game.Teams.Score` | `int` | Team goal count. |
| `Game.Teams.ColorPrimary` | `string` | Hex color code (no `#`) for the team's primary color. |
| `Game.Teams.ColorSecondary` | `string` | Hex color code for the team's secondary color. |
| `Game.TimeSeconds` | `int` | Seconds remaining in the match. |
| `Game.bOvertime` | `bool` | True if the match is in overtime. |
| `Game.Ball` | `object` | Current ball state. |
| `Game.Ball.Speed` | `float` | Current ball speed in Unreal Units/second. |
| `Game.Ball.TeamNum` | `int` | Index of the last team to touch the ball. `255` if the ball has not been touched. |
| `Game.bReplay` | `bool` | True if a goal replay or history replay is active. |
| `Game.bHasWinner` | `bool` | True if a team has won. |
| `Game.Winner` | `string` | Name of the winning team. Empty string if no winner yet. |
| `Game.Arena` | `string` | Asset name of the current map (e.g. `Stadium_P`). |
| `Game.bHasTarget` | `bool` | True if the client is currently viewing a specific vehicle. |
| `Game.Target` | `object` | CONDITIONAL. Player currently being viewed. Members are an empty string or `0` if the player does not have a spectator target. |
| `Game.Target.Name` | `string` | Name of the player being viewed. |
| `Game.Target.Shortcut` | `int` | Spectator shortcut of the viewed player. |
| `Game.Target.TeamNum` | `int` | Team index of the viewed player. |
| `Game.Frame` | `int` | CONDITIONAL. Current frame number if a replay is active. |
| `Game.Elapsed` | `float` | CONDITIONAL. Seconds elapsed since game start if a replay is active. |
