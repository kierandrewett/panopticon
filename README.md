# Panopticon

A GNOME Shell extension that tracks user activity and reports status to a remote server.

## How it works

Panopticon monitors three signals to determine if you're active:

- **Mouse movement** -- polled every 500ms
- **Keyboard input** -- any key press
- **Fullscreen apps** -- if the focused window is fullscreen, you're considered active even without input (useful for videos, games, etc.)

When no activity is detected for the configured idle timeout (default: 60 seconds), the extension sends an `idle` status to the server. Activity resumes the moment any of the above signals are detected.

Status is reported as a JSON POST request:

```json
{"status": 1, "device": "pc"}            // active
{"status": 0, "device": "pc"}            // idle
{"status": 1, "device": "phone", "ttl": 1800}  // active, auto-decays after 30 min
```

`device` identifies the source (defaults to `default` if omitted). The server tracks each device independently and reports the aggregate as `yes` if **any** device is active. `ttl` (seconds) is optional — useful for sources that only send "active" events (like a phone shortcut). After `ttl` elapses without another ping, the device is treated as idle.

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
| Device ID | Identifier sent with each ping so the server can distinguish this device | `pc` |
| Idle Timeout | Seconds of inactivity before reporting idle (10--600) | `60` |

The preferences window also shows the last 20 status pings with timestamps and success/failure indicators.

## iPhone Shortcut

To report presence from your phone (e.g. when you're home but away from the PC), create an iOS Shortcut with a *Get Contents of URL* action:

- **URL**: `https://is-kieran.drewett.dev/active`
- **Method**: `POST`
- **Headers**: `Authorization: Bearer <your token>`, `Content-Type: application/json`
- **Request Body** (JSON):
  ```json
  {"status": 1, "device": "phone", "ttl": 1800}
  ```

Trigger this from a *Personal Automation* (e.g. "When I arrive home"). The `ttl` ensures the phone falls back to idle automatically if the leave-home trigger doesn't fire. To report idle explicitly, send the same shortcut with `"status": 0`.

## Building

```sh
cd extension
zip -r panopticon@drewett.dev.shell-extension.zip \
  extension.js prefs.js metadata.json schemas/
```

## License

MIT
