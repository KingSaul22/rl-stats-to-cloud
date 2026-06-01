# ClockUpdatedSeconds

**Category:** Event

## Description
Sent when the in-game clock has changed.

## Example Payload
```json
{
  "Event": "ClockUpdatedSeconds",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6",
    "TimeSeconds": 180,
    "bOvertime": false
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `TimeSeconds` | `int` | Seconds remaining in the match. |
| `bOvertime` | `bool` | True if the game is in overtime. |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
