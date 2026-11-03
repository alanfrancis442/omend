# Privacy and local data

The application uses local sysfs, D-Bus and desktop/audio services. Its application
code does not implement an account service, cloud upload, telemetry collector or
automatic updater. This is a description of the current code, not a completed
security audit of all dependencies or of services on the host.

## Audio lighting

When the daemon's selected effect is Audio, the optional `omen-audio` user service
starts `parec` against `@DEFAULT_SINK@.monitor`: the default speaker/output monitor,
not a microphone source. This can include music, calls, notifications and any
other audio routed to that output. Audio-server routing is controlled by the host;
the application does not establish consent for other people whose audio is played.

Stereo PCM samples are analyzed in memory. The helper sends four amplitude levels
to the system daemon for lighting and exposes spectrum/peak/beat information over
the user's session D-Bus for the GUI. It does not intentionally save PCM recordings
or transmit them to an Internet service. Spectrum information is available to
other clients that can access that user's session bus; do not treat it as secret.
Memory may still be included in operating-system swap, crash dumps or debugging
tools according to host configuration.

Capture stops when Audio is deselected or the helper exits. Closing the GUI does
not stop audio lighting. To disable the background helper:

```sh
systemctl --user disable --now omen-audio.service
```

This changes only the user audio service, not cooling. See [audio lighting](audio-visualizer.md).

## Configuration and diagnostics

The system installation uses `/etc/omend/config.toml` for presets, saved curve
names, lighting and power choices. GUI preferences use
`$XDG_CONFIG_HOME/omen-gui/settings.json`, or `~/.config/omen-gui/settings.json`.
Hardware status and model information can be read through the daemon's permitted
D-Bus status methods. Read access is intentionally broader than control access.

Systemd journals can contain errors, paths and status information. Build metadata
and diagnostics may reveal kernel/BIOS versions, hardware model, local paths and
configuration names. Review and redact reports before sharing. Never include
serial numbers, tokens, complete unrelated journals, raw audio or private
configuration names in public issues. Source-navigation/editor tooling is
optional development tooling with its own configuration; it is not installed
by the application bundle.
