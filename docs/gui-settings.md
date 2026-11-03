# GUI settings

Open **Settings** in the sidebar, or the top tabs in a narrow window. Changes
preview immediately and save automatically after a brief delay. The GUI uses
`$XDG_CONFIG_HOME/omen-gui/settings.json`, falling back to
`~/.config/omen-gui/settings.json`.

- **Appearance:** solid, transparent, or frosted backgrounds; background opacity
  from 40–100%; five accent colors; text sizes from 90–120%; compact spacing.
  Background opacity does not dim text or controls. Frosted glass uses a softer
  background tint and stronger card surfaces.
- **Startup:** choose Dashboard, Fan curve, Lighting, Performance, or Settings,
  or reopen the last page instead.
- **Monitoring:** refresh the dashboard and daemon status every 0.5, 1, 2, or
  5 seconds. This does not change the daemon's fan-control interval or the
  lighting animation rate. Hardware commands still trigger a fresh status poll.
- **Lighting:** optionally remember the selected keyboard zone between sessions.
- **Reset to defaults:** restore GUI preferences while leaving fan curves,
  hardware profiles, and the daemon configuration intact.

Settings errors appear in the Preferences section. A malformed settings file
falls back to defaults and is replaced only when preferences are saved.

## Transparency and Hyprland blur

Select **Transparent** or **Frosted glass**, then reduce background opacity.
Slint updates the window's alpha channel while keeping foreground content opaque.
Hyprland supplies desktop blur. The GUI does not edit compositor configuration.
Its Wayland app ID / window class is `omen-gui`.

For a Hyprland configuration using `.conf` syntax, the **Copy blur example**
button copies this example to the clipboard:

```conf
decoration {
    blur {
        enabled = true
        size = 6
        passes = 3
    }
}
```

This enables blur globally. Review any window rules disabling blur or forcing
opaque surfaces for `omen-gui`. Compositor-level opacity also affects the final
appearance. For Lua configurations, use the corresponding options in your
compositor configuration.

See the official Hyprland documentation for
[blur options](https://wiki.hypr.land/0.54.0/Configuring/Variables/#blur) and
[window rules](https://wiki.hypr.land/0.54.0/Configuring/Window-Rules/). Syntax
depends on the installed Hyprland version.
