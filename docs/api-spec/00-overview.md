# Rocket League Stats API Overview

## Getting Started

This document outlines capabilities of the Rocket League Game Data API. First, players must ask the game to enable this feature by editing their `DefaultStatsAPI.ini`, explained below. Once active, this feature will open a web socket on the player's machine that emits gameplay data and events. Third party programs can ingest this data to power a variety of applications, such as custom broadcaster HUDs.

## Overview

The Stats API broadcasts JSON messages over a local socket while a match is in progress. Messages are sent both at a configurable periodic rate and when specific match events occur. Event data is always emitted on the same tick that the event occurs, regardless of the user's `PacketSendRate`.

Note: All configuration must be done before the client starts; changes to the ini while the client is running require a restart.

Field visibility markers used throughout the API:

- `CONDITIONAL`: Field is only present when relevant.
- `SPECTATOR`: Field is only present if the client is spectating or on the player's team.

## Configuration (`DefaultStatsAPI.ini`)

Edit `<Install Dir>\TAGame\Config\DefaultStatsAPI.ini` before launching the client.

| Setting | Type | Default | Description |
| --- | --- | --- | --- |
| `PacketSendRate` | `float` | `0 (disabled)` | Number of `UpdateState` packets broadcast per second. Must be > 0 to enable the websocket. Capped at 120. |
| `Port` | `int` | `49123` | Local port the socket listens on. |

## Message Format Envelope

Every message follows this envelope structure:

```json
{
  "Event": "EventName",
  "Data": { /* event-specific payload */ }
}
```
