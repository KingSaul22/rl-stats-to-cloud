# CrossbarHit

**Category:** Event

## Description
Sent when the ball hits a crossbar.

## Example Payload
```json
{
  "Event": "CrossbarHit",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6",
    "BallLocation": {
      "X": 120,
      "Y": -2944,
      "Z": 320
    },
    "BallSpeed": 870.3,
    "ImpactForce": 127.5,
    "BallLastTouch": {
      "Player": {
        "Name": "PlayerA",
        "Shortcut": 1,
        "TeamNum": 0
      },
      "Speed": 120
    }
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `BallSpeed` | `float` | Ball speed on impact. |
| `ImpactForce` | `float` | Impact force of the ball relative to the crossbar normal. |
| `BallLastTouch` | `object` | The last touch of the ball before the crossbar hit. |
| `BallLastTouch.Player` | `object` | The player who made the last touch. |
| `BallLastTouch.Player.Name` | `string` | Display name. |
| `BallLastTouch.Player.Shortcut` | `int` | Spectator shortcut. |
| `BallLastTouch.Player.TeamNum` | `int` | Team index (`0 = Blue`, `1 = Orange`). |
| `BallLastTouch.Speed` | `float` | Speed of the ball resulting from this hit. |
| `BallLocation` | `vector` | World position (`X`, `Y`, `Z`) of the ball when the impact occurred. |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
