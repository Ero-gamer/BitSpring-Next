# BitSpring local API

BitSpring on a computer can be controlled over the local network: by Home Assistant, a
Stream Deck, Apple Shortcuts, or a script of your own. It is the same small server the phone
remote uses.

- **Off by default.** It runs only while *Settings → Phone remote* is switched on, and stops
  when you switch it off.
- **Local network only.** BitSpring never opens it to the internet.
- **Needs the token.** Every request carries the token from the remote's address. *New
  address* in the remote's panel makes a new token: everything that had the old one loses
  access.
- **Plays only your library.** A request can't make BitSpring open a file that isn't in it.

This describes version 1. Addresses and fields listed here stay as they are; later versions of
BitSpring may add new ones.

## Address and token

Switch the remote on. BitSpring shows an address like

```
http://192.168.1.20:48173/?t=3f9c…e1
```

The part before `/?t=` is the server, the part after `t=` is the token. Send the token in any
of these ways:

```
Authorization: Bearer <token>
X-Token: <token>
…/api/v1/state?t=<token>
```

Without it the answer is `403` with `{"error":"token"}`.

```bash
curl -H "Authorization: Bearer $TOKEN" http://192.168.1.20:48173/api/v1/state
```

## Reading

### `GET /api/v1/info`

```json
{ "app": "BitSpring", "version": "0.21.1", "api": 1 }
```

### `GET /api/v1/state`

```json
{
  "state": "playing",
  "position": 61.5,
  "duration": 251.0,
  "buffering": false,
  "track": {
    "path": "/Music/Miles Davis/Kind of Blue/01 So What.flac",
    "title": "So What",
    "artist": "Miles Davis",
    "album": "Kind of Blue",
    "year": "1959",
    "duration": 251.0,
    "albumId": "8c1f0a9be2d34f10",
    "favorite": true,
    "rating": 4,
    "cover": "/api/v1/cover/current?rev=7"
  },
  "signal": {
    "output": "FiiO Q7",
    "sourceRate": 96000,
    "sourceBits": 24,
    "outputRate": 96000.0,
    "outputBits": 24,
    "bitPerfect": true,
    "lossy": false
  },
  "queue": { "index": 0, "length": 5 },
  "shuffle": false,
  "repeat": "off"
}
```

- `state`: `playing`, `paused` or `stopped`.
- `position`, `duration`: seconds.
- `track` and `signal` are `null` when nothing is loaded. `albumId` is `null` for a file that
  isn't in the library; `cover` is `null` when the file has no cover.
- `track.favorite` and `track.rating` (0 to 5 stars, 0: not rated) are the listener's marks
  (since 0.21.1). A file that isn't in the library has none: `false` and `0`.
- `signal.bitPerfect`: the samples reach the DAC untouched. `sourceRate` of a DSD file is its
  1-bit rate (2822400 for DSD64).
- `repeat`: `off`, `all` (the queue) or `one` (the track).

### `GET /api/v1/queue`

```json
{
  "index": 0,
  "tracks": [
    { "path": "/Music/…/01 So What.flac", "title": "So What", "artist": "Miles Davis", "duration": 251.0, "albumId": "8c1f0a9be2d34f10" }
  ]
}
```

### `GET /api/v1/library`

`{ "albums": [...], "tracks": [...] }`: every album and track of the library. A track's `path`
is what the queue commands take; an album's `tracks` are positions in the `tracks` list.

### `GET /api/v1/favorites`

Since 0.21.1. Everything the listener marked in the library: tracks by `path`, and the ids of
favourite albums. A track is listed when it is a favourite, has a rating, or both.

```json
{
  "tracks": {
    "/Music/Miles Davis/Kind of Blue/01 So What.flac": { "favorite": true, "rating": 5 },
    "/Music/Miles Davis/Kind of Blue/02 Freddie Freeloader.flac": { "favorite": false, "rating": 4 }
  },
  "albums": ["8c1f0a9be2d34f10"]
}
```

### `GET /api/v1/cover/<albumId>` and `GET /api/v1/cover/current`

The cover image of an album, or of the track that plays now. `404` when there is none.

### `GET /api/v1/volume`

`{ "volume": 40 }`, 0 to 100, when the output is a network player (UPnP, Chromecast) that has
a volume of its own. `{ "volume": null }` otherwise: BitSpring doesn't scale the samples it
sends to a DAC.

## Events

### `GET /api/v1/events`

A [Server-Sent Events](https://developer.mozilla.org/docs/Web/API/Server-sent_events) stream.
It sends the state (the same object as `GET state`) at once, and again whenever something
changes: another track, play or pause, a seek, the queue, shuffle or repeat, the playing
track's marks.

```
event: state
data: {"state":"playing","position":61.5, …}

```

The position is not sent while it simply runs on: count it from the last event. A comment line
(`: ping`) comes every 15 seconds of silence. At most 8 connections per client address.

```bash
curl -N -H "Authorization: Bearer $TOKEN" http://192.168.1.20:48173/api/v1/events
```

## Commands

All are `POST`. Those with values take a JSON body. The answer is `{"ok":true}`; a command
with missing or impossible values gets `400`.

| Address | Body | What it does |
| --- | --- | --- |
| `/api/v1/play` | | Plays: resumes, or starts the queue. |
| `/api/v1/pause` | | Pauses. |
| `/api/v1/toggle` | | Play or pause, whichever applies. |
| `/api/v1/stop` | | Stops and lets go of the output. |
| `/api/v1/next` | | Next track of the queue. |
| `/api/v1/previous` | | Previous track; more than 3 s into a track, back to its start. |
| `/api/v1/seek` | `{"position": 90}` | Jumps to that second of the track. |
| `/api/v1/shuffle` | `{"on": true}` | Shuffle on or off. Without a body: the other one. |
| `/api/v1/repeat` | `{"mode": "all"}` | `off`, `all` or `one`. Without a body: the next one. |
| `/api/v1/volume` | `{"volume": 40}` | The network player's volume, 0 to 100. |
| `/api/v1/queue/jump` | `{"index": 3}` | Plays that entry of the queue. |
| `/api/v1/queue/play` | `{"paths": [...], "index": 0}` | Replaces the queue with these library tracks and plays from `index`. |
| `/api/v1/queue/add` | `{"paths": [...]}` | Adds these library tracks to the end of the queue. |
| `/api/v1/favorite` | `{"path": "...", "on": true}` | Makes a library track a favourite, or no longer one. Without `on`: the other one. Without `path`: the track that plays. With `"albumId": "..."` instead of `path`: the album. Answers `{"ok":true,"favorite":true}`. Since 0.21.1. |
| `/api/v1/rating` | `{"path": "...", "rating": 4}` | Gives a library track 1 to 5 stars; 0 takes them away. Without `path`: the track that plays. Since 0.21.1. |

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" http://192.168.1.20:48173/api/v1/toggle
curl -X POST -H "Authorization: Bearer $TOKEN" -d '{"position": 90}' http://192.168.1.20:48173/api/v1/seek
```

Commands are carried out by the BitSpring window, the same way its own buttons are, so the
state changes a moment after the answer. Read it from `state` or wait for the event.
`favorite` and `rating` are the exception: the mark is saved before the answer comes. Both
stay on the device, like the marks made in the app.

## Home Assistant

```yaml
rest_command:
  bitspring:
    url: "http://192.168.1.20:48173/api/v1/{{ command }}"
    method: post
    headers:
      Authorization: "Bearer !secret bitspring_token"

sensor:
  - platform: rest
    name: BitSpring
    resource: http://192.168.1.20:48173/api/v1/state
    headers:
      Authorization: "Bearer !secret bitspring_token"
    value_template: "{{ value_json.state }}"
    json_attributes: [track, signal, position, duration]
    scan_interval: 5
```

Then `service: rest_command.bitspring` with `data: { command: toggle }`.

## Errors

| Status | Meaning |
| --- | --- |
| `400` | The command's values are missing or out of range, or a path isn't in the library. |
| `403` | No token, or not the current one. |
| `404` | No such address. |
| `405` | The address exists, with another method. |
| `409` | Nothing is playing (volume). |
