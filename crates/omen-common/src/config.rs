// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    Curve, CurveError, CustomMotion, CustomSequence, Effect, Preset, Profile, RgbColor, ZoneId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub thermal: ThermalConfig,
    pub rgb: RgbConfig,
    pub profile: Profile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalConfig {
    pub preset: Preset,
    /// Unvalidated opt-in: request hp-wmi maximum mode for recovery, not Auto.
    #[serde(default)]
    pub experimental_emergency_max: bool,
    /// How often the EC watchdog is re-armed, in seconds.
    pub watchdog_secs: u64,
    /// Spin-down delay, in seconds.
    pub hysteresis_secs: u64,
    pub poll_ms: u64,
    pub curve: Vec<PointConfig>,
    pub quiet: Vec<PointConfig>,
    pub balanced: Vec<PointConfig>,
    /// Named curves the window can recall.
    #[serde(default)]
    pub saved: Vec<SavedCurve>,
    /// Saved curve or built-in preset to run on wall power. Empty keeps the mode you picked.
    #[serde(default)]
    pub plugged_in: String,
    /// Saved curve or built-in preset to run on battery. Empty keeps the mode you picked.
    #[serde(default)]
    pub on_battery: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedCurve {
    pub name: String,
    #[serde(default = "default_saved_hysteresis")]
    pub hysteresis_secs: u64,
    pub points: Vec<PointConfig>,
}

fn default_saved_hysteresis() -> u64 {
    4
}

pub const MAX_SAVED_CURVES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointConfig {
    pub temp: u32,
    pub pwm: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RgbConfig {
    pub effect: Effect,
    pub brightness: u8,
    pub zones: ZoneColors,
    /// 1–10. Missing in older config files.
    #[serde(default = "default_rgb_speed")]
    pub speed: u8,
    /// 0–100. How far Breathing and Wave fade.
    #[serde(default = "default_rgb_depth")]
    pub depth: u8,
    /// 0–100. Color strength of Wave and Rainbow.
    #[serde(default = "default_rgb_saturation")]
    pub saturation: u8,
    /// Wave and Rainbow travel right-to-left.
    #[serde(default)]
    pub reverse: bool,
    /// How a custom sequence moves. Missing in older config files.
    #[serde(default)]
    pub custom_motion: CustomMotion,
    /// Two to eight colors played by the custom effect.
    #[serde(default = "default_custom_steps")]
    pub custom_steps: Vec<RgbColor>,
}

fn default_custom_steps() -> Vec<RgbColor> {
    CustomSequence::default().steps
}

fn default_rgb_speed() -> u8 {
    5
}

fn default_rgb_depth() -> u8 {
    80
}

fn default_rgb_saturation() -> u8 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneColors {
    pub wasd: RgbColor,
    pub left: RgbColor,
    pub center: RgbColor,
    pub right: RgbColor,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
        toml::from_str(&text).map_err(|err| format!("parse {}: {err}", path.display()))
    }

    /// Atomically replace a valid configuration and sync both the file and its
    /// directory. Existing owner, group and Unix mode are retained. Files with
    /// extended attributes (including ACL/security metadata) are refused rather
    /// than replacing them with broader or different access permissions.
    /// A failure after rename may mean the new file is visible but its survival
    /// across power loss could not be confirmed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|err| err.to_string())?;
        save_atomic(path, text.as_bytes()).map_err(|err| format!("save {}: {err}", path.display()))
    }

    /// `--config`, then `/etc/omend/config.toml`, then `./config/omend.toml`.
    pub fn resolve(explicit: Option<&Path>) -> (Self, Option<PathBuf>) {
        if let Some(path) = explicit {
            return match Self::load(path) {
                Ok(config) => (config, Some(path.to_path_buf())),
                Err(err) => {
                    tracing_fallback(&err);
                    (Self::default(), Some(path.to_path_buf()))
                }
            };
        }
        for candidate in [
            PathBuf::from("/etc/omend/config.toml"),
            PathBuf::from("config/omend.toml"),
        ] {
            if candidate.is_file() {
                match Self::load(&candidate) {
                    Ok(config) => return (config, Some(candidate)),
                    Err(err) => {
                        tracing_fallback(&err);
                        // Keep the intended destination. save() will refuse to
                        // replace malformed data until the owner repairs it.
                        return (Self::default(), Some(candidate));
                    }
                }
            }
        }
        (Self::default(), None)
    }

    pub fn custom_curve(&self) -> Result<Curve, CurveError> {
        curve_from(&self.thermal.curve)
    }

    pub fn quiet_curve(&self) -> Result<Curve, CurveError> {
        curve_from(&self.thermal.quiet)
    }

    pub fn balanced_curve(&self) -> Result<Curve, CurveError> {
        curve_from(&self.thermal.balanced)
    }

    pub fn hysteresis(&self) -> Duration {
        // Match the D-Bus setter's range so a hand-edited file cannot pin the
        // fans at a stale speed for an arbitrary time.
        Duration::from_secs(self.thermal.hysteresis_secs.clamp(1, 30))
    }

    pub fn watchdog(&self) -> Duration {
        Duration::from_secs(self.thermal.watchdog_secs.clamp(5, 60))
    }

    pub fn poll(&self) -> Duration {
        Duration::from_millis(self.thermal.poll_ms.clamp(200, 10_000))
    }

    /// Color stored for one zone.
    ///
    /// `ZoneId::All` is the setter's broadcast. The read returns that color only
    /// when every zone still holds it; mixed zones have no single color.
    pub fn zone_color(&self, zone: ZoneId) -> Option<RgbColor> {
        match zone {
            ZoneId::Wasd => Some(self.rgb.zones.wasd),
            ZoneId::Left => Some(self.rgb.zones.left),
            ZoneId::Center => Some(self.rgb.zones.center),
            ZoneId::Right => Some(self.rgb.zones.right),
            ZoneId::All => {
                let zones = &self.rgb.zones;
                let color = zones.left;
                (zones.wasd == color && zones.center == color && zones.right == color)
                    .then_some(color)
            }
        }
    }

    pub fn set_custom_curve(&mut self, curve: &Curve) {
        self.thermal.curve = curve
            .points()
            .iter()
            .map(|(temp, pwm)| PointConfig {
                temp: *temp,
                pwm: u32::from(*pwm),
            })
            .collect();
    }

    pub fn saved_curve(&self, name: &str) -> Option<&SavedCurve> {
        self.thermal
            .saved
            .iter()
            .find(|saved| saved.name.eq_ignore_ascii_case(name.trim()))
    }

    pub fn upsert_saved(
        &mut self,
        name: &str,
        curve: &Curve,
        hysteresis_secs: u64,
    ) -> Result<(), String> {
        let name = validate_curve_name(name)?;
        let points = curve
            .points()
            .iter()
            .map(|(temp, pwm)| PointConfig {
                temp: *temp,
                pwm: u32::from(*pwm),
            })
            .collect();
        let hysteresis_secs = hysteresis_secs.clamp(1, 30);
        if let Some(existing) = self
            .thermal
            .saved
            .iter_mut()
            .find(|saved| saved.name.eq_ignore_ascii_case(&name))
        {
            existing.name = name;
            existing.points = points;
            existing.hysteresis_secs = hysteresis_secs;
            return Ok(());
        }
        if self.thermal.saved.len() >= MAX_SAVED_CURVES {
            return Err(format!("only {MAX_SAVED_CURVES} saved curves fit"));
        }
        self.thermal.saved.push(SavedCurve {
            name,
            hysteresis_secs,
            points,
        });
        Ok(())
    }

    pub fn delete_saved(&mut self, name: &str) -> bool {
        let before = self.thermal.saved.len();
        self.thermal
            .saved
            .retain(|saved| !saved.name.eq_ignore_ascii_case(name.trim()));
        let removed = self.thermal.saved.len() != before;
        if removed {
            if self.thermal.plugged_in.eq_ignore_ascii_case(name.trim()) {
                self.thermal.plugged_in.clear();
            }
            if self.thermal.on_battery.eq_ignore_ascii_case(name.trim()) {
                self.thermal.on_battery.clear();
            }
        }
        removed
    }

    /// Empty clears the source. A saved name or `quiet` / `balanced` / `max` / `auto` is stored.
    pub fn resolve_power_name(&self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(String::new());
        }
        if let Some(saved) = self.saved_curve(name) {
            return Ok(saved.name.clone());
        }
        match Preset::parse(name) {
            Some(Preset::Custom) => {
                Err("pick a saved curve, or quiet, balanced, max, or auto".into())
            }
            Some(preset) => Ok(preset.as_str().to_string()),
            None => Err(format!("unknown fan setting '{name}'")),
        }
    }

    pub fn set_zone_color(&mut self, zone: ZoneId, color: RgbColor) {
        match zone {
            ZoneId::All => {
                self.rgb.zones.wasd = color;
                self.rgb.zones.left = color;
                self.rgb.zones.center = color;
                self.rgb.zones.right = color;
            }
            ZoneId::Wasd => self.rgb.zones.wasd = color,
            ZoneId::Left => self.rgb.zones.left = color,
            ZoneId::Center => self.rgb.zones.center = color,
            ZoneId::Right => self.rgb.zones.right = color,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            thermal: ThermalConfig {
                preset: Preset::Balanced,
                experimental_emergency_max: false,
                watchdog_secs: 15,
                hysteresis_secs: 4,
                poll_ms: 1000,
                curve: points(&[(40, 70), (55, 110), (70, 170), (80, 220), (90, 255)]),
                quiet: points(&[(45, 45), (60, 80), (75, 130), (85, 190), (95, 255)]),
                balanced: points(&[(40, 70), (55, 110), (70, 170), (80, 220), (90, 255)]),
                saved: Vec::new(),
                plugged_in: String::new(),
                on_battery: String::new(),
            },
            rgb: RgbConfig {
                effect: Effect::Static,
                brightness: 160,
                zones: ZoneColors {
                    wasd: RgbColor::new(255, 40, 20),
                    left: RgbColor::new(255, 40, 20),
                    center: RgbColor::new(180, 20, 40),
                    right: RgbColor::new(255, 80, 20),
                },
                speed: default_rgb_speed(),
                depth: default_rgb_depth(),
                saturation: default_rgb_saturation(),
                reverse: false,
                custom_motion: CustomMotion::Cycle,
                custom_steps: default_custom_steps(),
            },
            profile: Profile::Balanced,
        }
    }
}

static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);
fn save_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::other("configuration path needs a filename"))?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    reject_symlink_directories(parent)?;
    fs::create_dir_all(parent)?;
    reject_symlink_directories(parent)?;
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(parent)?;
    // Anchor every remaining operation to the opened directory. A concurrent
    // rename of a containing directory cannot redirect the destination.
    let anchored = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let destination = anchored.join(filename);
    let previous = existing_config(&destination)?;
    let mut temporary = None;
    let mut file = None;
    for _ in 0..128 {
        let candidate = anchored.join(format!(
            ".omend-config-{}-{}.tmp",
            std::process::id(),
            NEXT_SAVE.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(opened) => {
                temporary = Some(candidate);
                file = Some(opened);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let temporary = temporary
        .ok_or_else(|| io::Error::other("cannot allocate a unique configuration staging file"))?;
    let mut file = file.expect("staging file was opened");
    let result = (|| {
        file.write_all(bytes)?;
        if let Some(metadata) = &previous {
            let staged = file.metadata()?;
            if staged.uid() != metadata.uid() || staged.gid() != metadata.gid() {
                // Refer to the already opened file, not a name another writer
                // could replace in an untrusted configuration directory.
                std::os::unix::fs::chown(
                    format!("/proc/self/fd/{}", file.as_raw_fd()),
                    Some(metadata.uid()),
                    Some(metadata.gid()),
                )?;
            }
            file.set_permissions(fs::Permissions::from_mode(metadata.mode() & 0o777))?;
        }
        file.sync_all()?;
        // Do not clobber a configuration changed while we staged the update.
        let current = existing_config(&destination)?;
        let unchanged = match (&previous, &current) {
            (None, None) => true,
            (Some(before), Some(now)) => {
                before.dev() == now.dev()
                    && before.ino() == now.ino()
                    && before.mtime() == now.mtime()
                    && before.mtime_nsec() == now.mtime_nsec()
                    && before.ctime() == now.ctime()
                    && before.ctime_nsec() == now.ctime_nsec()
                    && before.len() == now.len()
            }
            _ => false,
        };
        if !unchanged {
            return Err(io::Error::other(
                "configuration changed during save; retry after reviewing it",
            ));
        }
        fs::rename(&temporary, &destination)?;
        directory.sync_all().map_err(|error| {
            io::Error::other(format!(
                "configuration replaced, but directory sync failed; durability unconfirmed: {error}"
            ))
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn existing_config(path: &Path) -> io::Result<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::other(
            "refusing to replace a symlink, non-regular or hard-linked configuration",
        ));
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let opened = file.metadata()?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(io::Error::other("configuration changed while opening it"));
    }
    reject_extended_attributes(&file)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    toml::from_str::<Config>(&text).map_err(|_| io::Error::other(
        "existing configuration is malformed; repair or move it before saving (original preserved)"
    ))?;
    Ok(Some(opened))
}

fn reject_extended_attributes(file: &fs::File) -> io::Result<()> {
    // SAFETY: the fd is borrowed from a live File; a null buffer with length
    // zero asks Linux for the required size without writing through a pointer.
    let count = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
    if count > 0 {
        return Err(io::Error::other(
            "existing configuration has extended attributes (ACL/security metadata); save refused because atomic replacement cannot preserve them",
        ));
    }
    if count < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EOPNOTSUPP) {
            return Err(error);
        }
    }
    Ok(())
}

fn reject_symlink_directories(parent: &Path) -> io::Result<()> {
    let absolute = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()?.join(parent)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::other(
                    "configuration directories must not be symlinks",
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(io::Error::other("configuration parent is not a directory"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn points(raw: &[(u32, u32)]) -> Vec<PointConfig> {
    raw.iter()
        .map(|(temp, pwm)| PointConfig {
            temp: *temp,
            pwm: *pwm,
        })
        .collect()
}

fn validate_curve_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 24 {
        return Err("curve name needs 1–24 characters".into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == ' ' || ch == '-' || ch == '_')
    {
        return Err("curve name can use letters, numbers, spaces, - and _".into());
    }
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "quiet" | "balanced" | "max" | "auto" | "bios" | "custom" | "manual"
    ) {
        return Err(format!("'{name}' is reserved"));
    }
    Ok(name.to_string())
}

fn curve_from(points: &[PointConfig]) -> Result<Curve, CurveError> {
    Curve::new(points.iter().map(|point| (point.temp, point.pwm)).collect())
}

fn tracing_fallback(err: &str) {
    eprintln!("omend config: {err}; using built-in defaults");
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SaveFixture(PathBuf);

    impl SaveFixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "omend-config-{}-{}",
                std::process::id(),
                NEXT_SAVE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory)
        }

        fn assert_no_staging_files(&self) {
            assert!(!fs::read_dir(&self.0).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".omend-config-")
            }));
        }
    }

    impl Drop for SaveFixture {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn atomic_save_round_trips_and_preserves_access_permissions_and_owner() {
        let fixture = SaveFixture::new();
        let path = fixture.0.join("config.toml");
        let mut config = Config::default();
        config.save(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let before = fs::metadata(&path).unwrap();
        config.thermal.hysteresis_secs = 9;
        config.save(&path).unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_ne!(before.ino(), after.ino());
        assert_eq!(after.mode() & 0o777, 0o640);
        assert_eq!(before.uid(), after.uid());
        assert_eq!(before.gid(), after.gid());
        assert_eq!(Config::load(&path).unwrap().thermal.hysteresis_secs, 9);
        fixture.assert_no_staging_files();
    }

    #[test]
    fn malformed_configuration_is_preserved_after_fallback_and_save_attempt() {
        let fixture = SaveFixture::new();
        let path = fixture.0.join("config.toml");
        let original = "[thermal\npreset = broken";
        fs::write(&path, original).unwrap();
        let (config, destination) = Config::resolve(Some(&path));
        assert_eq!(destination.as_deref(), Some(path.as_path()));
        assert!(config.save(&path).unwrap_err().contains("malformed"));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
        fixture.assert_no_staging_files();
    }

    #[test]
    fn symlinks_hardlinks_and_non_regular_destinations_are_not_replaced() {
        use std::os::unix::fs::symlink;
        let fixture = SaveFixture::new();
        let config = Config::default();
        let original = fixture.0.join("original.toml");
        config.save(&original).unwrap();
        let bytes = fs::read(&original).unwrap();
        let link = fixture.0.join("link.toml");
        symlink(&original, &link).unwrap();
        assert!(config.save(&link).is_err());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let hardlink = fixture.0.join("hardlink.toml");
        fs::hard_link(&original, &hardlink).unwrap();
        assert!(config.save(&hardlink).is_err());
        assert!(config.save(&original).is_err());
        let directory = fixture.0.join("directory.toml");
        fs::create_dir(&directory).unwrap();
        assert!(config.save(&directory).is_err());
        assert_eq!(fs::read(original).unwrap(), bytes);
        fixture.assert_no_staging_files();
    }

    #[test]
    fn symlinked_parent_is_rejected_without_writing_through_it() {
        let fixture = SaveFixture::new();
        let actual = fixture.0.join("actual");
        let linked = fixture.0.join("linked");
        fs::create_dir(&actual).unwrap();
        std::os::unix::fs::symlink(&actual, &linked).unwrap();
        assert!(Config::default().save(&linked.join("config.toml")).is_err());
        assert_eq!(fs::read_dir(actual).unwrap().count(), 0);
    }

    #[test]
    fn staging_permission_failure_preserves_existing_configuration() {
        let fixture = SaveFixture::new();
        let path = fixture.0.join("config.toml");
        let mut config = Config::default();
        config.save(&path).unwrap();
        let bytes = fs::read(&path).unwrap();
        config.thermal.hysteresis_secs = 9;
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o500)).unwrap();
        // Root bypasses DAC. Normal developer and CI test runs exercise this.
        if fs::metadata("/proc/self").unwrap().uid() != 0 {
            assert!(config.save(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            fixture.assert_no_staging_files();
        }
    }

    #[test]
    fn write_limit_failure_preserves_previous_file_and_removes_partial_staging() {
        const TARGET: &str = "OMEN_TEST_CONFIG_WRITE_LIMIT_TARGET";
        if let Some(path) = std::env::var_os(TARGET) {
            let path = PathBuf::from(path);
            let original = fs::read(&path).unwrap();
            let mut config = Config::load(&path).unwrap();
            // Exceed the child's 1 KiB RLIMIT_FSIZE. SIGXFSZ is ignored by
            // the parent shell so write_all returns EFBIG instead of killing
            // the child before Rust can run the failure cleanup.
            config.rgb.custom_steps = vec![RgbColor::new(1, 2, 3); 1024];
            let error = config.save(&path).unwrap_err();
            assert!(error.contains("File too large"), "{error}");
            assert_eq!(fs::read(path).unwrap(), original);
            return;
        }
        let fixture = SaveFixture::new();
        let path = fixture.0.join("config.toml");
        Config::default().save(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let output = std::process::Command::new("bash")
            .args(["-c", "trap '' XFSZ; ulimit -f 1; exec \"$@\"", "omend-save-test"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "config::tests::write_limit_failure_preserves_previous_file_and_removes_partial_staging", "--nocapture"])
            .env(TARGET, &path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(path).unwrap(), original);
        fixture.assert_no_staging_files();
    }

    #[test]
    fn extended_attributes_are_not_silently_discarded() {
        let fixture = SaveFixture::new();
        let path = fixture.0.join("config.toml");
        let config = Config::default();
        config.save(&path).unwrap();
        let bytes = fs::read(&path).unwrap();
        let file = fs::File::open(&path).unwrap();
        let name = c"user.omend-test-metadata";
        let value = b"preserve me";
        // SAFETY: name is NUL-terminated, the fd is live, and value points to
        // value.len() initialized bytes for the duration of the syscall.
        let result = unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            assert_eq!(error.raw_os_error(), Some(libc::EOPNOTSUPP), "{error}");
            return;
        }
        assert!(
            config
                .save(&path)
                .unwrap_err()
                .contains("extended attributes")
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
        let mut stored = vec![0_u8; value.len()];
        // SAFETY: the fd/name are still valid and stored is writable for its
        // advertised capacity, which is the exact expected attribute size.
        let length = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                stored.as_mut_ptr().cast(),
                stored.len(),
            )
        };
        assert_eq!(length, value.len() as isize);
        assert_eq!(stored, value);
        fixture.assert_no_staging_files();
    }

    #[test]
    fn sample_file_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/omend.toml");
        let config = Config::load(&path).expect("sample config");
        assert_eq!(config.profile, crate::Profile::Balanced);
        assert!(config.balanced_curve().is_ok());
    }

    #[test]
    fn all_zone_color_matches_only_when_every_zone_agrees() {
        let mut config = Config::default();
        assert!(config.zone_color(ZoneId::All).is_none());
        assert_eq!(config.zone_color(ZoneId::Left), Some(config.rgb.zones.left));

        let shared = RgbColor::new(1, 2, 3);
        config.set_zone_color(ZoneId::All, shared);
        assert_eq!(config.zone_color(ZoneId::All), Some(shared));
        assert_eq!(config.zone_color(ZoneId::Wasd), Some(shared));
        assert_eq!(config.zone_color(ZoneId::Right), Some(shared));
    }

    #[test]
    fn older_config_without_saved_curves_still_parses() {
        let text = r#"
profile = "balanced"

[thermal]
preset = "balanced"
watchdog_secs = 15
hysteresis_secs = 4
poll_ms = 1000

[[thermal.curve]]
temp = 40
pwm = 70

[[thermal.curve]]
temp = 90
pwm = 255

[[thermal.quiet]]
temp = 45
pwm = 45

[[thermal.quiet]]
temp = 95
pwm = 255

[[thermal.balanced]]
temp = 40
pwm = 70

[[thermal.balanced]]
temp = 90
pwm = 255

[rgb]
effect = "static"
brightness = 160

[rgb.zones.wasd]
r = 1
g = 2
b = 3

[rgb.zones.left]
r = 1
g = 2
b = 3

[rgb.zones.center]
r = 1
g = 2
b = 3

[rgb.zones.right]
r = 1
g = 2
b = 3
"#;
        let config: Config = toml::from_str(text).unwrap();
        assert!(!config.thermal.experimental_emergency_max);
        assert!(config.thermal.saved.is_empty());
        assert!(config.thermal.plugged_in.is_empty());
        assert!(config.thermal.on_battery.is_empty());
        assert_eq!(config.rgb.custom_motion, CustomMotion::Cycle);
        assert_eq!(config.rgb.custom_steps.len(), 4);
    }

    #[test]
    fn experimental_recovery_requires_opt_in_and_round_trips() {
        let mut config = Config::default();
        assert!(!config.thermal.experimental_emergency_max);
        config.thermal.experimental_emergency_max = true;
        let text = toml::to_string(&config).unwrap();
        let loaded: Config = toml::from_str(&text).unwrap();
        assert!(loaded.thermal.experimental_emergency_max);
    }

    #[test]
    fn saved_curves_round_trip_and_power_names_resolve() {
        let mut config = Config::default();
        let curve = Curve::new(vec![(43, 35), (90, 255)]).unwrap();
        config.upsert_saved("Gaming", &curve, 6).unwrap();
        config.upsert_saved("gaming", &curve, 8).unwrap();
        assert_eq!(config.thermal.saved.len(), 1);
        assert_eq!(config.saved_curve("Gaming").unwrap().hysteresis_secs, 8);
        assert_eq!(config.resolve_power_name("quiet").unwrap(), "quiet");
        assert_eq!(config.resolve_power_name("Gaming").unwrap(), "gaming");
        assert!(config.resolve_power_name("custom").is_err());
        assert!(config.upsert_saved("quiet", &curve, 4).is_err());

        let text = toml::to_string(&config).unwrap();
        let loaded: Config = toml::from_str(&text).unwrap();
        assert_eq!(loaded.thermal.saved, config.thermal.saved);

        config.thermal.plugged_in = "gaming".into();
        assert!(config.delete_saved("Gaming"));
        assert!(config.thermal.plugged_in.is_empty());
        assert!(config.thermal.saved.is_empty());
    }

    #[test]
    fn hand_edited_hysteresis_is_bounded() {
        let mut config = Config::default();
        config.thermal.hysteresis_secs = u64::MAX;
        assert_eq!(config.hysteresis(), Duration::from_secs(30));
        config.thermal.hysteresis_secs = 0;
        assert_eq!(config.hysteresis(), Duration::from_secs(1));
    }

    #[test]
    fn default_curves_are_valid() {
        let config = Config::default();
        assert!(config.custom_curve().is_ok());
        assert!(config.quiet_curve().is_ok());
        assert!(config.balanced_curve().is_ok());
        assert!(config.balanced_curve().unwrap().pwm_at(60) > 80);
    }
}
