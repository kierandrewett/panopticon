# Panopticon

A GNOME Shell extension that tracks user activity and reports status to a remote server.

## How it works

Panopticon monitors three signals to determine if you're active:

- **Mouse movement** -- polled every 500ms
- **Keyboard input** -- any key press
- **Fullscreen apps** -- if the focused window is fullscreen, you're considered active even without input (useful for videos, games, etc.)

The model is **poll-based**: every device should ping while it considers itself active. The server delists any device it hasn't heard from for 90 seconds. The public answer is `yes` while any device is still listed.

The GNOME extension pings every 30 seconds while you're active. When idle (no input for the configured timeout, default 60s) it sends an explicit delist:

```json
{"device": "pc"}              // active refresh (status=1 implicit)
{"device": "pc", "status": 0} // explicit delist (immediate, optional)
```

`device` identifies the source. If omitted it falls back to `default`. Devices that just go silent are dropped after 90s — the explicit `status: 0` only exists to flip the public answer to `no` faster when going idle on a PC.

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

To report presence from your phone, create an iOS Shortcut with a *Get Contents of URL* action and run it on a schedule (e.g. every minute) via a *Personal Automation*:

- **URL**: `https://is-kieran.drewett.dev/active?device=phone`
- **Method**: `POST`
- **Headers**: `Authorization: Bearer <your token>`

The device id can be passed via the `?device=` query string (no body needed) or as JSON `{"device": "phone"}`. As long as the shortcut fires at least once every 90 seconds the phone stays listed.

## Building

```sh
cd extension
zip -r panopticon@drewett.dev.shell-extension.zip \
  extension.js prefs.js metadata.json schemas/
```

## License

MIT
