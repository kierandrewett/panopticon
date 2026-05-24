import Adw from 'gi://Adw';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk';

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

export default class PanopticonPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();

        const page = new Adw.PreferencesPage();
        const group = new Adw.PreferencesGroup({
            title: 'Server',
            description: 'Configure the activity reporting server',
        });
        page.add(group);

        // URL
        const urlRow = new Adw.EntryRow({
            title: 'Server URL',
            text: settings.get_string('url'),
        });
        urlRow.connect('changed', () => {
            settings.set_string('url', urlRow.get_text());
        });
        group.add(urlRow);

        // Token
        const tokenRow = new Adw.PasswordEntryRow({
            title: 'Bearer Token',
            text: settings.get_string('token'),
        });
        tokenRow.connect('changed', () => {
            settings.set_string('token', tokenRow.get_text());
        });
        group.add(tokenRow);

        // Device ID
        const hostname = GLib.get_host_name();
        const deviceRow = new Adw.EntryRow({
            title: `Device ID (default: ${hostname})`,
            text: settings.get_string('device-id'),
        });
        deviceRow.connect('changed', () => {
            settings.set_string('device-id', deviceRow.get_text());
        });
        group.add(deviceRow);

        // Force ping button
        const pingRow = new Adw.ActionRow({
            title: 'Send Ping Now',
            subtitle: 'Force-resend the current status to the server',
        });
        const pingButton = new Gtk.Button({
            label: 'Send',
            valign: Gtk.Align.CENTER,
        });
        pingButton.add_css_class('suggested-action');
        pingButton.connect('clicked', () => {
            settings.set_uint('force-ping', settings.get_uint('force-ping') + 1);
        });
        pingRow.add_suffix(pingButton);
        pingRow.set_activatable_widget(pingButton);
        group.add(pingRow);

        // Idle timeout
        const timeoutRow = new Adw.SpinRow({
            title: 'Idle Timeout',
            subtitle: 'Seconds of inactivity before reporting idle',
            adjustment: new Gtk.Adjustment({
                lower: 10,
                upper: 600,
                step_increment: 5,
                value: settings.get_uint('idle-timeout'),
            }),
        });
        timeoutRow.connect('notify::value', () => {
            settings.set_uint('idle-timeout', timeoutRow.get_value());
        });
        group.add(timeoutRow);

        // Recent Pings
        const pingsGroup = new Adw.PreferencesGroup({
            title: 'Recent Pings',
            description: 'Last 20 status reports sent to the server',
        });
        page.add(pingsGroup);

        const buildPingRows = () => {
            // Remove existing rows
            let child = pingsGroup.get_first_child();
            while (child) {
                const next = child.get_next_sibling();
                if (child instanceof Adw.ActionRow)
                    pingsGroup.remove(child);
                child = next;
            }

            const raw = settings.get_string('recent-pings');
            let pings = [];
            try {
                pings = JSON.parse(raw || '[]');
            } catch (_) {}

            if (pings.length === 0) {
                const emptyRow = new Adw.ActionRow({
                    title: 'No pings recorded yet',
                });
                pingsGroup.add(emptyRow);
                return;
            }

            for (const ping of pings) {
                const date = new Date(ping.timestamp);
                const timeStr = date.toLocaleTimeString([], {
                    hour: '2-digit',
                    minute: '2-digit',
                    second: '2-digit',
                });
                const dateStr = date.toLocaleDateString();
                const statusLabel = ping.status === 1 ? 'Active' : 'Idle';
                const resultIcon = ping.success ? '✓' : '✗';

                const row = new Adw.ActionRow({
                    title: `${resultIcon}  ${statusLabel}`,
                    subtitle: `${dateStr} ${timeStr}`,
                });
                pingsGroup.add(row);
            }
        };

        buildPingRows();
        settings.connect('changed::recent-pings', () => buildPingRows());

        window.add(page);
    }
}
