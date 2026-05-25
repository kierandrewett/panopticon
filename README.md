# Panopticon

A GNOME Shell extension that tracks user activity and reports status to a remote server.

## How it works

Panopticon monitors three signals to determine if you're active:

- **Mouse movement** -- polled every 500ms
- **Keyboard input** -- any key press
- **Fullscreen apps** -- if the focused window is fullscreen, you're considered active even without input (useful for videos, games, etc.)

The model is **poll-based**: every device should ping while it considers itself active. The server delists any device it hasn't heard from within its TTL (default 90 seconds). The public answer is `yes` while any device is still listed.

The GNOME extension pings every 30 seconds while you're active. When idle (no input for the configured timeout, default 60s) it sends an explicit delist:

```json
{"device": "pc"}              // active refresh (status=1 implicit)
{"device": "pc", "status": 0} // explicit delist (immediate, optional)
```

`device` identifies the source (falls back to `default`). `ttl` (seconds) overrides how long this ping keeps the device listed — useful for low-frequency pollers like a phone that only fires hourly:

```
POST /active?device=phone&ttl=7200
```

## Webhooks

Set `WEBHOOK_URL` on the server and Panopticon will POST a JSON event to it whenever any device transitions on or off:

```json
{
  "device": "ac",
  "event": "on" | "off",
  "reason": "ping" | "explicit" | "expired",
  "aggregate_active": true,
  "timestamp": "2026-05-25T17:26:05.759137452+00:00"
}
```

| `event` / `reason` | Fired when |
|---|---|
| `on` / `ping` | A previously-absent device sent its first ping |
| `off` / `explicit` | A device sent `status=0` |
| `off` / `expired` | A device's TTL elapsed without a refresh ping (covers unplugged-at-the-wall, lost-WiFi, manual off, schedule-off — anything that stops the device from pinging) |

Refresh pings for an already-listed device do **not** fire a webhook.

## Requirements

- GNOME Shell 47, 48, or 49
- `curl` (used for HTTP requests)

## Installation

### From zip

```sh
gnome-extensions install panopticon@drewett.dev.shell-extension.zip
```

Then log out and back in (or restart GNOME Shell with `Alt+F2` -> `r` on X11).

### From source

```sh
cd extension
glib-compile-schemas schemas/
gnome-extensions install --force .
```

## Configuration

Open the extension preferences via GNOME Extensions app or:

```sh
gnome-extensions prefs panopticon@drewett.dev
```

| Setting | Description | Default |
|---------|-------------|---------|
| Server URL | URL to POST status updates to | `https://is-kieran.drewett.dev/active` |
| Bearer Token | Authorization token sent in the `Authorization` header | *(empty)* |
| Device ID | Identifier sent with each ping so the server can distinguish this device | *(hostname)* |
| Idle Timeout | Seconds of inactivity before reporting idle (10--600) | `60` |

The preferences window also shows the last 20 status pings with timestamps and success/failure indicators.

## iPhone Shortcut

To report presence from your phone, create one Shortcut and three Personal Automations.

### The Shortcut

In the *Shortcuts* app, tap *+* to create a new shortcut named e.g. **"Panopticon Active"**, then add a single *Get Contents of URL* action:

- **URL**: `https://is-kieran.drewett.dev/active?device=phone&ttl=7200`
- **Method**: `POST`
- **Headers**: `Authorization: Bearer <your token>`

(`ttl=7200` keeps the phone listed for 2 hours per ping, so an hourly poll comfortably stays alive.)

Make a near-identical second shortcut named **"Panopticon Idle"** with the URL `…/active?device=phone&status=0` for explicit delist.

### The Automations

In *Shortcuts → Automation → +*:

1. **When I Arrive Home** → run *Panopticon Active*. (Disable "Ask Before Running".)
2. **When I Leave Home** → run *Panopticon Idle*.
3. **Time of Day, every hour, repeat Daily** → a small wrapper shortcut that first uses *Get Current Location*, then *If* the distance to your home address is less than ~200m, run *Panopticon Active*. Otherwise do nothing.

The arrive/leave pair gives instant feedback; the hourly check refreshes the TTL while you're at home so the server doesn't expire you.

## Building

```sh
cd extension
zip -r panopticon@drewett.dev.shell-extension.zip \
  extension.js prefs.js metadata.json schemas/
```

## License

MIT
