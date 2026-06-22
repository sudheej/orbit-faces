# Orbital Local Bridge Protocol v0

Orbital Local Bridge v0 is a local, bidirectional WebSocket connection between
one face host and one backend runtime. The default mock endpoint is
`ws://127.0.0.1:7373`.

Messages are UTF-8 JSON WebSocket text messages. Each message has a `type`
field. Unknown message types are logged and ignored.

This protocol is intentionally local and minimal. It does not define
authentication, encryption, discovery, remote access, permissions, delivery
guarantees, or message persistence.

## Runtime to face host

### State

```json
{
  "type": "state",
  "state": "thinking",
  "emotion": "focused",
  "caption": "Looking at your terminal...",
  "audio_level": 0.72
}
```

`state` is required. `emotion`, `caption`, and `audio_level` are optional. If a
face pack does not declare the requested state, the host warns and falls back
to `idle`.

### Config

```json
{
  "type": "config",
  "always_on_top": true,
  "debug_overlay": false
}
```

Both config fields are optional, but at least one must be present.

### Ping

```json
{
  "type": "ping",
  "id": "abc123"
}
```

The host replies with a `pong` carrying the same ID.

## Face host to runtime

### Ready

Sent after every successful connection or reconnection.

```json
{
  "type": "face.ready",
  "face": "Basic Orb",
  "version": "0.1"
}
```

### Click

Coordinates are local to the face window.

```json
{
  "type": "face.clicked",
  "x": 120,
  "y": 96,
  "button": "left"
}
```

### Double click

```json
{
  "type": "face.double_clicked",
  "x": 120,
  "y": 96,
  "button": "left"
}
```

The v0 host also sends the `toggle_listening` action for a left-button double
click.

### Drag completed

`x` and `y` are the final desktop position represented by the host's window
position or layer-shell margins.

```json
{
  "type": "face.dragged",
  "x": 300,
  "y": 220
}
```

### Action

```json
{
  "type": "face.action",
  "action": "toggle_listening"
}
```

### Pong

```json
{
  "type": "pong",
  "id": "abc123"
}
```

Runtime v0 also accepts a JSON `ping` from a face client and answers with a
matching `pong`, although the current face host does not initiate these pings.

## Connection behavior

- The face host retries a failed connection every two seconds.
- The face host remains open when the bridge is unavailable.
- A reconnect sends a new `face.ready` event.
- Events generated while disconnected may remain queued until reconnection.
- The mock runtime replays its scripted state sequence for each connection.
- Orbital Runtime v0 remains listening when a face disconnects and accepts the
  next face-host connection.

WebSocket transport-level ping/pong frames are also handled, independently of
the JSON `ping` and `pong` messages above.
