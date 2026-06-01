# MatchPaused

**Category:** Event

## Description
Sent when the game is paused by a match admin.

## Example Payload
```json
{
  "Event": "MatchPaused",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6"
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
