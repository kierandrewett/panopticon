import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Clutter from 'gi://Clutter';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {getPointerWatcher} from 'resource:///org/gnome/shell/ui/pointerWatcher.js';

export default class PanopticonExtension extends Extension {
    _settings = null;
    _pointerListener = null;
    _idleTimeoutId = null;
    _keyPressId = null;
    _isActive = false;
    _lastActivity = 0;

    enable() {
        this._settings = this.getSettings();

        this._lastActivity = GLib.get_monotonic_time();
        this._isActive = false;

        // Track mouse movement via pointerWatcher (polls at interval in ms)
        this._pointerListener = getPointerWatcher().addWatch(
            500,
            () => this._onActivity(),
        );

        // Track keyboard via stage key events
        const stage = global.stage;
        this._keyPressId = stage.connect('key-press-event', () => {
            this._onActivity();
            return Clutter.EVENT_PROPAGATE;
        });

        // Check for idle periodically (every 5 seconds)
        this._idleTimeoutId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            5,
            () => {
                this._checkIdle();
                return GLib.SOURCE_CONTINUE;
            },
        );

        // Report active on enable
        this._onActivity();
    }

    disable() {
        if (this._pointerListener) {
            this._pointerListener.remove();
            this._pointerListener = null;
        }

        if (this._keyPressId) {
            global.stage.disconnect(this._keyPressId);
            this._keyPressId = null;
        }

        if (this._idleTimeoutId) {
            GLib.source_remove(this._idleTimeoutId);
            this._idleTimeoutId = null;
        }

        this._settings = null;
    }

    _onActivity() {
        this._lastActivity = GLib.get_monotonic_time();

        if (!this._isActive) {
            this._isActive = true;
            this._sendStatus(1);
        }
    }

    _checkIdle() {
        if (this._hasFullscreenWindow()) {
            this._onActivity();
            return;
        }

        const now = GLib.get_monotonic_time();
        const idleTimeout = this._settings.get_uint('idle-timeout');
        const elapsed = (now - this._lastActivity) / 1_000_000;

        if (this._isActive && elapsed >= idleTimeout) {
            this._isActive = false;
            this._sendStatus(0);
        }
    }

    _hasFullscreenWindow() {
        const focusedWindow = global.display.get_focus_window();
        return focusedWindow !== null && focusedWindow.is_fullscreen();
    }

    _sendStatus(status) {
        const url = this._settings.get_string('url');
        const token = this._settings.get_string('token');
        const device = this._settings.get_string('device-id') || 'pc';

        if (!url || !token) {
            console.warn('[Panopticon] URL or token not configured');
            return;
        }

        const body = JSON.stringify({status, device});
        const timestamp = new Date().toISOString();

        // Use Gio.Subprocess with curl — reliable across GNOME versions
        // without worrying about libsoup 2 vs 3.
        try {
            const proc = Gio.Subprocess.new(
                [
                    'curl', '-s', '-X', 'POST', url,
                    '-H', `Authorization: Bearer ${token}`,
                    '-H', 'Content-Type: application/json',
                    '-d', body,
                ],
                Gio.SubprocessFlags.STDOUT_SILENCE |
                    Gio.SubprocessFlags.STDERR_SILENCE,
            );
            proc.wait_async(null, (proc, res) => {
                try {
                    proc.wait_finish(res);
                    this._logPing(timestamp, status, proc.get_successful());
                } catch (e) {
                    console.error(`[Panopticon] curl failed: ${e.message}`);
                    this._logPing(timestamp, status, false);
                }
            });
        } catch (e) {
            console.error(`[Panopticon] Failed to send status: ${e.message}`);
            this._logPing(timestamp, status, false);
        }
    }

    _logPing(timestamp, status, success) {
        try {
            const raw = this._settings.get_string('recent-pings');
            const pings = JSON.parse(raw || '[]');
            pings.unshift({timestamp, status, success});
            // Keep last 20 entries
            if (pings.length > 20)
                pings.length = 20;
            this._settings.set_string('recent-pings', JSON.stringify(pings));
        } catch (e) {
            console.error(`[Panopticon] Failed to log ping: ${e.message}`);
        }
    }
}
