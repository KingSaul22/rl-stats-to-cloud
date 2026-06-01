# StatfeedEvent

**Category:** Event

## Description
Sent when someone earns a stat.

## Example Payload
```json
{
  "Event": "StatfeedEvent",
  "Data": {
    "MatchGuid": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6",
    "EventName": "Demolish",
    "Type": "Demolition",
    "MainTarget": {
      "Name": "PlayerA",
      "Shortcut": 1,
      "TeamNum": 0
    },
    "SecondaryTarget": {
      "Name": "PlayerB",
      "Shortcut": 2,
      "TeamNum": 1
    }
  }
}
```

## Field Dictionary

| Field | Type | Description |
| --- | --- | --- |
| `EventName` | `string` | Asset name of the StatEvent (e.g. `Demolish`, `Save`). |
| `Type` | `string` | Localized display label for the stat (e.g. `Demolition`). |
| `MainTarget` | `object` | Player who earned the stat. |
| `MainTarget.Name` | `string` | Display name. |
| `MainTarget.Shortcut` | `int` | Spectator shortcut. |
| `MainTarget.TeamNum` | `int` | Team index (`0 = Blue`, `1 = Orange`). |
| `MatchGuid` | `string` | Only set for online or LAN matches. |
| `SecondaryTarget` | `object` | CONDITIONAL. Player involved in the stat (e.g. the demolished player). Same shape as `MainTarget`. |
