# Audio lighting

Choose **Lighting → Audio**. The `omen-audio` background user service captures the
default speaker monitor with `parec` (PulseAudio or PipeWire's PulseAudio
compatibility service), not the microphone. It sends audio levels directly to
`omend`, so lighting continues after the GUI closes. The GUI reads the same
spectrum from the service for its preview; it does not own audio capture.

The service starts at login. It captures only while the daemon's selected effect
is Audio, stops capture for other effects, and retries if the audio server or
default output changes. Response changes are read from the daemon, so they also
work when changed through the CLI. One session-bus owner prevents multiple GUI
windows from creating competing audio producers.

The keyboard's four hardware zones follow four frequency ranges:

| Frequency | Zone |
| --- | --- |
| Bass, 20–180 Hz | Left |
| Low mids, 180–700 Hz | WASD |
| Mids, 700–3,000 Hz | Center |
| Treble, 3,000–10,000 Hz | Right |

**Reverse** reverses this assignment. The sixteen spectrum bars show smaller
bands within those ranges, with falling peak markers and a bass-onset indicator.
The keyboard remains a four-zone device; it does not have per-key spectrum bars.

- Pick colors, use the wheel, paint keys, or press Apply without leaving Audio.
  Each zone keeps its chosen hue while its brightness follows the music.
- **Response** controls attack and release: higher values respond and decay
  faster; lower values sustain longer.
- **Contrast** controls how deeply quiet passages dim the lighting. Use a high
  value for pronounced beats and a lower value for a steady background glow.
- **Brightness** sets the overall ceiling, and **Color** adjusts saturation.

Analysis uses overlapping stereo FFT windows and a shared level reference, so
notes between individual sampled frequencies are included and each band retains
its dynamics. Anti-phase stereo does not disappear in a mono downmix. Silence,
capture failure, or stale input clears the visualization. At nonzero brightness,
the daemon keeps the lamp at its minimum active brightness so it can respond to
the next beat.

## Update

Build the background helper, GUI, and daemon:

```sh
cargo build --release -p omen-audio -p omen-gui -p omend --locked
```

Install and enable the audio helper as your normal desktop user (no sudo):

```sh
sh packaging/install-audio-service.sh
```

The installer puts the helper in `~/.local/bin/omen-audio`, installs
`omen-audio.service` in your systemd user configuration, and enables it at login.
It also restarts the helper when updating an existing installation. It does not
alter fan curves, the system daemon, or compositor configuration.

For an existing system installation, update the daemon if needed, then relaunch
the GUI to use the service-backed preview:

```sh
sudo install -m 755 target/release/omend /usr/bin/omend
sudo systemctl restart omend
./target/release/omen-gui
```

The new GUI also restores the active effect after color writes to older daemons
that implicitly selected Static. Updating `omend` additionally enables exact hue
preservation, the revised brightness response, and stale-input expiry on hardware.

## Manage or troubleshoot background audio

```sh
systemctl --user status omen-audio.service
journalctl --user -u omen-audio.service -n 50
systemctl --user restart omen-audio.service
```

Stopping the service stops live audio lighting without affecting other effects:

```sh
systemctl --user disable --now omen-audio.service
```

The GUI reports a missing service in the Audio panel. Capture failures clear the
live levels and retry automatically. If the helper cannot reach `omend`, the
hardware daemon's stale-input timeout prevents a frozen bright frame. You can
also run `./target/release/omen-audio` in the foreground for development, after
stopping the user service to avoid competing for its session-bus name.
