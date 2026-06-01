# GoalScored

**Category:** Event

## Description
Sent when a goal is scored.

## Example Payload
```json
{
  "Event": "GoalScored",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6",
    "GoalSpeed": 87.3,
    "GoalTime": 127.5,
    "ImpactLocation": {
      "X": 0,
      "Y": -2944,
      "Z": 320
    },
    "Scorer": {
      "Name": "PlayerA",
      "Shortcut": 1,
      "TeamNum": 0
    },
    "Assister": {
      "Name": "PlayerC",
      "Shortcut": 3,
      "TeamNum": 0
    },
    "BallLastTouch": {
      "Player": {
        "Name": "PlayerA",
        "Shortcut": 1,
        "TeamNum": 0
      },
      "Speed": 125
    }
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `GoalSpeed` | `float` | Speed of the ball (Unreal Units/second) when it crossed the goal line. |
| `GoalTime` | `float` | Length of the previous round in seconds. |
| `ImpactLocation` | `vector` | World position (`X`, `Y`, `Z`) of the ball when the goal was scored. |
| `Scorer` | `object` | The player who scored the goal. |
| `Scorer.Name` | `string` | Display name of the scorer. |
| `Scorer.Shortcut` | `int` | Spectator shortcut. |
| `Scorer.TeamNum` | `int` | Team index of the scorer. |
| `BallLastTouch` | `object` | The last touch of the ball before the goal. |
| `BallLastTouch.Player` | `object` | The player who made the last touch. |
| `BallLastTouch.Player.Name` | `string` | Name of the player who last touched the ball. |
| `BallLastTouch.Player.Shortcut` | `int` | Spectator shortcut. |
| `BallLastTouch.Player.TeamNum` | `int` | Team index. |
| `BallLastTouch.Speed` | `float` | Speed of the ball resulting from this touch. |
| `Assister` | `object` | CONDITIONAL. Same shape as `Scorer`. Present only when an assist was recorded. |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
