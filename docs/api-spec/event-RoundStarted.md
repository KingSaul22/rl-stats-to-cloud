# RoundStarted

**Category:** Event

## Description
Sent when the game enters the active state (after the countdown finishes).

## Example Payload
```json
{
  "Event": "RoundStarted",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6"
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
