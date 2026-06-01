# BallHit

**Category:** Event

## Description
Sent one frame after the ball is hit.

## Example Payload
```json
{
  "Event": "BallHit",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6",
    "Players": [
      {
        "Name": "PlayerA",
        "Shortcut": 1,
        "TeamNum": 0
      }
    ],
    "Ball": {
      "PreHitSpeed": 0,
      "PostHitSpeed": 1450.2,
      "Location": {
        "X": -512,
        "Y": 100,
        "Z": 200
      }
    }
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `Players` | `array` | Players that hit the ball that frame. |
| `Players.Name` | `string` | Display name. |
| `Players.Shortcut` | `int` | Spectator shortcut. |
| `Players.TeamNum` | `int` | Team index (`0 = Blue`, `1 = Orange`). |
| `Ball` | `object` | Ball state at the moment of the hit. |
| `Ball.PreHitSpeed` | `float` | Ball speed before the hit (Unreal Units/second). |
| `Ball.PostHitSpeed` | `float` | Ball speed after the hit (Unreal Units/second). |
| `Ball.Location` | `vector` | World position (`X`, `Y`, `Z`) of the ball at impact. |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
