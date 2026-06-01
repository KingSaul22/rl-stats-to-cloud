# GoalReplayWillEnd

**Category:** Event

## Description
Sent when the ball explodes during a goal replay. If the replay is skipped this event will not fire.

## Example Payload
```json
{
  "Event": "GoalReplayWillEnd",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6"
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
