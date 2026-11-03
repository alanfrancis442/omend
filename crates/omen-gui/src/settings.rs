// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::cell::RefCell;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use slint::ComponentHandle;

use crate::AppWindow;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub background_mode: i32,
    pub opacity: u8,
    pub accent: i32,
    pub text_size: i32,
    pub compact: bool,
    pub startup_page: i32,
    pub remember_page: bool,
    pub refresh_ms: u64,
    pub remember_zone: bool,
    pub last_page: i32,
    pub last_zone: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            background_mode: 0,
            opacity: 90,
            accent: 0,
            text_size: 1,
            compact: false,
            startup_page: 0,
            remember_page: false,
            refresh_ms: 1000,
            remember_zone: false,
            last_page: 0,
            last_zone: "all".into(),
        }
    }
}

impl Settings {
    fn validate(mut self) -> Self {
        self.background_mode = self.background_mode.clamp(0, 2);
        self.opacity = self.opacity.clamp(40, 100);
        self.accent = self.accent.clamp(0, 4);
        self.text_size = self.text_size.clamp(0, 3);
        self.startup_page = self.startup_page.clamp(0, 4);
        self.last_page = self.last_page.clamp(0, 4);
        if ![500, 1000, 2000, 5000].contains(&self.refresh_ms) {
            self.refresh_ms = 1000;
        }
        if !["all", "wasd", "left", "center", "right"].contains(&self.last_zone.as_str()) {
            self.last_zone = "all".into();
        }
        self
    }

    fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes)
                .map(Self::validate)
                .map_err(io::Error::other),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err),
        }
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("invalid settings path"))?;
        fs::create_dir_all(parent)?;
        // Write beside the destination, then rename, so interrupted writes do
        // not destroy the last complete settings file.
        let temporary = parent.join(format!(".settings-{}.tmp", std::process::id()));
        // Never follow or truncate a pre-existing entry at the temporary path
        // (for example a stale symlink); replace a stale file, then create anew.
        match fs::symlink_metadata(&temporary) {
            Ok(metadata) if !metadata.is_dir() => fs::remove_file(&temporary)?,
            _ => {}
        }
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    fn from_ui(ui: &AppWindow) -> Self {
        let refresh_ms = [500, 1000, 2000, 5000][ui.get_refresh_choice().clamp(0, 3) as usize];
        Self {
            background_mode: ui.get_background_mode(),
            opacity: ui.get_window_opacity().round() as u8,
            accent: ui.get_accent_choice(),
            text_size: ui.get_text_size(),
            compact: ui.get_compact_spacing(),
            startup_page: ui.get_startup_page(),
            remember_page: ui.get_remember_page(),
            refresh_ms,
            remember_zone: ui.get_remember_zone(),
            last_page: ui.get_page(),
            last_zone: ui.get_zone().to_string(),
        }
        .validate()
    }

    pub fn apply(&self, ui: &AppWindow, startup: bool) {
        ui.set_preferences_ready(false);
        ui.set_background_mode(self.background_mode);
        ui.set_window_opacity(self.opacity as f32);
        ui.set_accent_choice(self.accent);
        ui.set_text_size(self.text_size);
        ui.set_compact_spacing(self.compact);
        ui.set_startup_page(self.startup_page);
        ui.set_remember_page(self.remember_page);
        ui.set_refresh_choice(
            [500, 1000, 2000, 5000]
                .iter()
                .position(|value| *value == self.refresh_ms)
                .unwrap_or(1) as i32,
        );
        ui.set_remember_zone(self.remember_zone);
        if startup {
            ui.set_page(if self.remember_page {
                self.last_page
            } else {
                self.startup_page
            });
        }
        ui.set_zone(
            if self.remember_zone {
                self.last_zone.clone()
            } else {
                "all".into()
            }
            .into(),
        );
        ui.set_preferences_ready(true);
    }
}

fn settings_path() -> io::Result<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| io::Error::other("XDG_CONFIG_HOME and HOME are unavailable"))?;
    Ok(config.join("omen-gui/settings.json"))
}

pub(crate) struct Preferences {
    path: Option<PathBuf>,
    saved: Rc<RefCell<Option<Settings>>>,
    save_timer: Rc<slint::Timer>,
}

impl Preferences {
    pub fn load(ui: &AppWindow) -> Self {
        let path = settings_path();
        let loaded = path
            .as_ref()
            .map_err(|err| err.to_string())
            .and_then(|path| Settings::load(path).map_err(|err| err.to_string()));
        let settings = loaded.as_ref().cloned().unwrap_or_default();
        settings.apply(ui, true);
        if let Ok(path) = &path {
            ui.set_preferences_path(path.display().to_string().into());
        }
        if let Err(err) = &loaded {
            ui.set_preferences_message(
                format!("Could not load settings: {err}. Using defaults.").into(),
            );
        }
        Self {
            path: path.ok(),
            saved: Rc::new(RefCell::new(loaded.ok())),
            save_timer: Rc::new(slint::Timer::default()),
        }
    }

    pub fn wire(&self, ui: &AppWindow, monitor: Rc<slint::Timer>, refresh: Arc<AtomicU64>) {
        let weak = ui.as_weak();
        let path = self.path.clone();
        let saved = Rc::clone(&self.saved);
        let save_timer = Rc::clone(&self.save_timer);
        ui.on_preferences_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            let settings = Settings::from_ui(&ui);
            refresh.store(settings.refresh_ms, Ordering::Relaxed);
            let interval = Duration::from_millis(settings.refresh_ms);
            if monitor.interval() != interval {
                monitor.set_interval(interval);
            }
            if saved.borrow().as_ref() == Some(&settings) {
                return;
            }
            ui.set_preferences_message("Saving…".into());
            let weak = ui.as_weak();
            let path = path.clone();
            let saved = Rc::clone(&saved);
            save_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(300),
                move || {
                    if let Some(ui) = weak.upgrade() {
                        save_ui(&ui, path.as_deref(), &saved);
                    }
                },
            );
        });

        let weak = ui.as_weak();
        ui.on_reset_preferences(move || {
            if let Some(ui) = weak.upgrade() {
                Settings::default().apply(&ui, false);
                ui.invoke_preferences_changed();
            }
        });

        let weak = ui.as_weak();
        ui.on_copy_blur_setup(move || {
            let example = r#"# Hyprland .conf syntax. This enables compositor blur globally.
# Pick Transparent or Frosted glass in omend Settings.
decoration {
    blur {
        enabled = true
        size = 6
        passes = 3
    }
}
# omend app ID: omen-gui
# Remove any matching no_blur / noblur or force_rgbx / forcergbx rules.
"#;
            let result = std::process::Command::new("wl-copy").arg(example).status();
            if let Some(ui) = weak.upgrade() {
                let message = match result {
                    Ok(status) if status.success() => {
                        "Blur example copied to clipboard".to_string()
                    }
                    Ok(status) => {
                        format!("Could not copy blur example: wl-copy exited with {status}")
                    }
                    Err(err) => format!("Could not copy blur example: {err}"),
                };
                ui.set_preferences_message(message.into());
            }
        });
    }

    pub fn flush(&self, ui: &AppWindow) {
        if self.save_timer.running() {
            self.save_timer.stop();
            save_ui(ui, self.path.as_deref(), &self.saved);
        }
    }
}

fn save_ui(ui: &AppWindow, path: Option<&Path>, saved: &RefCell<Option<Settings>>) {
    let settings = Settings::from_ui(ui);
    let result = path
        .ok_or_else(|| io::Error::other("no user configuration directory"))
        .and_then(|path| settings.save(path));
    match result {
        Ok(()) => {
            *saved.borrow_mut() = Some(settings);
            ui.set_preferences_message("All changes saved".into());
        }
        Err(err) => ui.set_preferences_message(format!("Could not save settings: {err}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_changes_save_restore_and_reset_without_changing_the_open_page() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};

        struct TestPlatform(Rc<MinimalSoftwareWindow>);
        impl slint::platform::Platform for TestPlatform {
            fn create_window_adapter(
                &self,
            ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
                Ok(self.0.clone())
            }
        }

        let adapter = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(TestPlatform(adapter.clone()))).unwrap();
        let ui = AppWindow::new().unwrap();
        let settings = Settings {
            background_mode: 2,
            opacity: 70,
            accent: 2,
            text_size: 3,
            compact: true,
            startup_page: 1,
            remember_page: true,
            last_page: 4,
            remember_zone: true,
            last_zone: "wasd".into(),
            ..Settings::default()
        };
        settings.apply(&ui, true);
        assert_eq!(ui.get_page(), 4);
        assert_eq!(ui.get_zone().as_str(), "wasd");
        let directory =
            std::env::temp_dir().join(format!("omen-live-settings-{}", std::process::id()));
        let path = directory.join("settings.json");
        let preferences = Preferences {
            path: Some(path.clone()),
            saved: Rc::new(RefCell::new(None)),
            save_timer: Rc::new(slint::Timer::default()),
        };
        let monitor = Rc::new(slint::Timer::default());
        monitor.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {});
        let refresh = Arc::new(AtomicU64::new(1000));
        preferences.wire(&ui, monitor.clone(), refresh.clone());
        ui.show().unwrap();
        adapter.set_size(slint::PhysicalSize::new(640, 700));
        let render = || {
            let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(640, 700);
            adapter.request_redraw();
            adapter.draw_if_needed(|renderer| {
                renderer.render(pixels.make_mut_slice(), 640);
            });
        };
        render();
        assert!(ui.global::<crate::Theme>().get_frosted());
        assert!((ui.global::<crate::Theme>().get_text_scale() - 1.2).abs() < 0.001);
        ui.set_refresh_choice(0);
        ui.set_window_opacity(68.0);
        render();
        assert_eq!(refresh.load(Ordering::Relaxed), 500);
        assert_eq!(monitor.interval(), Duration::from_millis(500));
        preferences.flush(&ui);
        let saved = Settings::load(&path).unwrap();
        assert_eq!(saved.opacity, 68);
        assert_eq!(saved.last_page, 4);
        assert_eq!(saved.last_zone, "wasd");
        assert_eq!(saved.refresh_ms, 500);

        ui.invoke_reset_preferences();
        render();
        preferences.flush(&ui);
        assert_eq!(ui.get_page(), 4);
        assert_eq!(ui.get_background_mode(), 0);
        assert_eq!(ui.get_zone().as_str(), "all");
        assert!(!ui.global::<crate::Theme>().get_frosted());
        assert_eq!(refresh.load(Ordering::Relaxed), 1000);
        assert_eq!(Settings::load(&path).unwrap().opacity, 90);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn older_and_invalid_settings_keep_usable_defaults() {
        let settings: Settings = serde_json::from_str(
            r#"{"opacity":1,"accent":99,"refresh_ms":0,"last_zone":"bad","startup_page":-1}"#,
        )
        .unwrap();
        let settings = settings.validate();
        assert_eq!(settings.opacity, 40);
        assert_eq!(settings.accent, 4);
        assert_eq!(settings.refresh_ms, 1000);
        assert_eq!(settings.last_zone, "all");
        assert_eq!(settings.startup_page, 0);
        assert_eq!(settings.text_size, Settings::default().text_size);
    }

    #[test]
    fn settings_round_trip_and_failed_save_preserves_previous_file() {
        let directory =
            std::env::temp_dir().join(format!("omen-settings-test-{}", std::process::id()));
        let path = directory.join("settings.json");
        let settings = Settings {
            background_mode: 2,
            refresh_ms: 5000,
            remember_page: true,
            last_page: 2,
            remember_zone: true,
            last_zone: "wasd".into(),
            ..Settings::default()
        };
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        let temporary = directory.join(format!(".settings-{}.tmp", std::process::id()));
        fs::create_dir(&temporary).unwrap();
        assert!(Settings::default().save(&path).is_err());
        assert_eq!(Settings::load(&path).unwrap(), settings);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_settings_are_reported_without_overwriting_them() {
        let path =
            std::env::temp_dir().join(format!("omen-bad-settings-{}.json", std::process::id()));
        fs::write(&path, b"{broken").unwrap();
        assert!(Settings::load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
        fs::remove_file(path).unwrap();
    }
}
