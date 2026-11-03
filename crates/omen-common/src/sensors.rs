// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use crate::{RgbColor, ZoneId};

/// One poll of the temperatures and fans the daemon acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readings {
    pub cpu_c: Option<u32>,
    pub gpu_c: Option<u32>,
    pub fan1_rpm: Option<u32>,
    pub fan2_rpm: Option<u32>,
    pub pwm: Option<u32>,
    pub pwm_enable: Option<u32>,
    pub profile: Option<String>,
}

impl Readings {
    pub fn control_temp(&self) -> Option<u32> {
        match (self.cpu_c, self.gpu_c) {
            (Some(cpu), Some(gpu)) => Some(cpu.max(gpu)),
            (Some(cpu), None) => Some(cpu),
            (None, Some(gpu)) => Some(gpu),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone)]
struct LedNode {
    zone: ZoneId,
    dir: PathBuf,
}

/// Sysfs handles discovered once at startup. hwmon indices move between boots,
/// so discovery is by device name (`hp`, `k10temp`, `amdgpu`) rather than number.
#[derive(Debug, Clone)]
pub struct Sensors {
    pub product: String,
    fan_pwm: Option<PathBuf>,
    fan_enable: Option<PathBuf>,
    fan1: Option<PathBuf>,
    fan2: Option<PathBuf>,
    cpu_temp: Option<PathBuf>,
    gpu_temp: Option<PathBuf>,
    profile: Option<PathBuf>,
    profile_choices: Option<PathBuf>,
    leds: Vec<LedNode>,
    pub fan_writable: bool,
    writes_enabled: bool,
    emergency_max_recovery: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareReport {
    pub product: String,
    pub hp_hwmon: Option<String>,
    pub cpu_c: Option<u32>,
    pub gpu_c: Option<u32>,
    pub fan1_rpm: Option<u32>,
    pub fan2_rpm: Option<u32>,
    pub pwm: Option<u32>,
    pub pwm_enable: Option<u32>,
    pub fan_writable: bool,
    pub profile: Option<String>,
    pub profile_choices: Vec<String>,
    pub leds: Vec<String>,
    pub notes: Vec<String>,
}

impl HardwareReport {
    pub fn format(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("product: {}", self.product));
        lines.push(format!(
            "hp-wmi hwmon: {}",
            self.hp_hwmon.as_deref().unwrap_or("missing")
        ));
        lines.push(format!(
            "cpu: {}    gpu: {}",
            fmt_c(self.cpu_c),
            fmt_c(self.gpu_c)
        ));
        lines.push(format!(
            "fan1: {}    fan2: {}    pwm: {}    pwm_enable: {}",
            fmt_rpm(self.fan1_rpm),
            fmt_rpm(self.fan2_rpm),
            self.pwm
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".into()),
            self.pwm_enable
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".into())
        ));
        lines.push(format!(
            "fan control writable: {}",
            if self.fan_writable { "yes" } else { "no" }
        ));
        lines.push(format!(
            "platform profile: {}    choices: {}",
            self.profile.as_deref().unwrap_or("missing"),
            if self.profile_choices.is_empty() {
                "none".to_string()
            } else {
                self.profile_choices.join(" ")
            }
        ));
        if self.leds.is_empty() {
            lines.push("keyboard leds: none (no hp::kbd_zoned_* under /sys/class/leds)".into());
        } else {
            lines.push(format!("keyboard leds: {}", self.leds.join(", ")));
        }
        for note in &self.notes {
            lines.push(format!("note: {note}"));
        }
        lines.join("\n")
    }
}

impl Sensors {
    pub fn discover() -> Self {
        Self::discover_in(Path::new("/sys"))
    }

    /// Discover a sysfs tree, also usable with an isolated filesystem fixture.
    /// The daemon always uses `discover()`; this is not a runtime override.
    pub fn discover_in(sysfs: &Path) -> Self {
        let product = read_trimmed(&sysfs.join("class/dmi/id/product_name"))
            .unwrap_or_else(|| "unknown".into());
        let hwmon = sysfs.join("class/hwmon");
        let hp = find_hwmon(&hwmon, "hp");
        let fan_pwm = hp.as_ref().map(|dir| dir.join("pwm1"));
        let fan_enable = hp.as_ref().map(|dir| dir.join("pwm1_enable"));
        let fan1 = hp.as_ref().map(|dir| dir.join("fan1_input"));
        let fan2 = hp.as_ref().map(|dir| dir.join("fan2_input"));
        let cpu_temp = find_temp(&hwmon, &["k10temp", "coretemp"]);
        let gpu_temp = find_temp(&hwmon, &["amdgpu", "nvidia"]);
        let profile = existing(sysfs.join("firmware/acpi/platform_profile"));
        let profile_choices = existing(sysfs.join("firmware/acpi/platform_profile_choices"));
        let leds = discover_leds(&sysfs.join("class/leds"));
        let fan_writable = fan_pwm.as_ref().is_some_and(|path| can_write(path))
            && fan_enable.as_ref().is_some_and(|path| can_write(path));

        Self {
            product,
            fan_pwm: fan_pwm.filter(|path| path.exists()),
            fan_enable: fan_enable.filter(|path| path.exists()),
            fan1: fan1.filter(|path| path.exists()),
            fan2: fan2.filter(|path| path.exists()),
            cpu_temp,
            gpu_temp,
            profile,
            profile_choices,
            leds,
            fan_writable,
            writes_enabled: true,
            emergency_max_recovery: false,
        }
    }

    /// A session daemon can observe hardware but cannot write it.
    pub fn read_only(mut self) -> Self {
        self.writes_enabled = false;
        self.fan_writable = false;
        self
    }

    /// Configure recovery without issuing hardware writes. Off by default until
    /// maximum-mode behavior has been validated on the target hardware.
    pub fn with_emergency_max_recovery(mut self, enabled: bool) -> Self {
        self.emergency_max_recovery = enabled;
        self
    }

    pub fn fan_recovery_available(&self) -> bool {
        self.writes_enabled && self.fan_enable.is_some()
    }

    pub fn fan_recovery_name(&self) -> &'static str {
        if self.emergency_max_recovery {
            "experimental maximum-fan mode"
        } else {
            "firmware Auto"
        }
    }

    /// hp-wmi mode 0 is hardware maximum, distinct from manual PWM 255.
    /// A successful request does not establish physical fan response.
    pub fn request_emergency_max(&self) -> Result<(), String> {
        self.require_writes()?;
        let enable = self
            .fan_enable
            .as_ref()
            .ok_or_else(|| "pwm1_enable is not present".to_string())?;
        write_sysfs(enable, "0")
    }

    /// Recovery needs only the mode node, including when PWM data is unusable.
    pub fn recover_fans(&self) -> Result<(), String> {
        if self.emergency_max_recovery {
            self.request_emergency_max()
        } else {
            self.apply_fan(false, 0)
        }
    }

    /// A sensor discovered at startup must remain readable. Hardware with only
    /// one supported temperature sensor can use that sensor alone.
    pub fn control_temp(&self, readings: &Readings) -> Option<u32> {
        if (self.cpu_temp.is_some() && readings.cpu_c.is_none())
            || (self.gpu_temp.is_some() && readings.gpu_c.is_none())
        {
            return None;
        }
        readings.control_temp()
    }

    fn require_writes(&self) -> Result<(), String> {
        if self.writes_enabled {
            Ok(())
        } else {
            Err("session daemon is read-only; use the system daemon for hardware control".into())
        }
    }

    pub fn report(&self) -> HardwareReport {
        let readings = self.read();
        let mut notes = Vec::new();
        if self.fan_pwm.is_none() {
            notes.push(
                "hp-wmi did not expose pwm1. Fan curves need a kernel with hp-wmi hwmon.".into(),
            );
        } else if readings.fan1_rpm.is_some_and(|rpm| rpm < 800)
            && readings.cpu_c.is_some_and(|cpu| cpu >= 55)
            && readings.pwm_enable == Some(2)
        {
            notes.push(
                "fans are idling while the CPU is warm and pwm1_enable is 2 (firmware auto). omend takes manual control by writing pwm1_enable=1."
                    .into(),
            );
        }
        if !self.fan_writable && self.fan_pwm.is_some() {
            notes.push("pwm1 is not writable by this user. Run omend as root.".into());
        }
        if self.leds.is_empty() {
            notes.push(
                "RGB sysfs is absent. Zone effects are stored and applied when an hp keyboard LED driver is loaded."
                    .into(),
            );
        }
        HardwareReport {
            product: self.product.clone(),
            hp_hwmon: self
                .fan_pwm
                .as_ref()
                .or(self.fan_enable.as_ref())
                .and_then(|path| path.parent().map(|dir| dir.display().to_string())),
            cpu_c: readings.cpu_c,
            gpu_c: readings.gpu_c,
            fan1_rpm: readings.fan1_rpm,
            fan2_rpm: readings.fan2_rpm,
            pwm: readings.pwm,
            pwm_enable: readings.pwm_enable,
            fan_writable: self.fan_writable,
            profile: readings.profile,
            profile_choices: self.profile_choices(),
            leds: self
                .leds
                .iter()
                .map(|led| format!("{} ({})", led.dir.display(), led.zone.as_str()))
                .collect(),
            notes,
        }
    }

    pub fn rgb_available(&self) -> bool {
        !self.leds.is_empty()
    }

    pub fn profile_writable(&self) -> bool {
        self.writes_enabled && self.profile.as_ref().is_some_and(|path| can_write(path))
    }

    pub fn read(&self) -> Readings {
        Readings {
            cpu_c: self.cpu_temp.as_deref().and_then(read_millicelsius),
            gpu_c: self.gpu_temp.as_deref().and_then(read_millicelsius),
            fan1_rpm: self.fan1.as_deref().and_then(read_u32),
            fan2_rpm: self.fan2.as_deref().and_then(read_u32),
            pwm: self.fan_pwm.as_deref().and_then(read_u32),
            pwm_enable: self.fan_enable.as_deref().and_then(read_u32),
            profile: self.profile.as_deref().and_then(read_trimmed),
        }
    }

    pub fn profile_choices(&self) -> Vec<String> {
        self.profile_choices
            .as_deref()
            .and_then(read_trimmed)
            .map(|text| text.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// `manual` writes `pwm1_enable=1` then the duty. Firmware auto is `pwm1_enable=2`.
    pub fn apply_fan(&self, manual: bool, pwm: u8) -> Result<(), String> {
        self.require_writes()?;
        let enable = self
            .fan_enable
            .as_ref()
            .ok_or_else(|| "pwm1_enable is not present".to_string())?;
        if manual {
            let result = self
                .fan_pwm
                .as_ref()
                .ok_or_else(|| "pwm1 is not present".to_string())
                .and_then(|pwm_path| {
                    write_sysfs(enable, "1").and_then(|()| write_sysfs(pwm_path, &pwm.to_string()))
                });
            if let Err(error) = result {
                let recovery_name = self.fan_recovery_name();
                return match self.recover_fans() {
                    Ok(()) => Err(format!(
                        "{error}; requested {recovery_name}; cooling unverified"
                    )),
                    Err(recovery) => Err(format!(
                        "{error}; {recovery_name} recovery request also failed: {recovery}"
                    )),
                };
            }
        } else {
            write_sysfs(enable, "2")?;
        }
        Ok(())
    }

    pub fn apply_profile(&self, profile: &str) -> Result<(), String> {
        self.require_writes()?;
        let path = self
            .profile
            .as_ref()
            .ok_or_else(|| "platform_profile is not present".to_string())?;
        let choices = self.profile_choices();
        if !choices.is_empty() && !choices.iter().any(|choice| choice == profile) {
            return Err(format!(
                "profile {profile} is not offered (choices: {})",
                choices.join(" ")
            ));
        }
        write_sysfs(path, profile)
    }

    pub fn apply_rgb(&self, frames: &[(ZoneId, RgbColor, u8)]) -> Result<(), String> {
        self.require_writes()?;
        if self.leds.is_empty() {
            return Err(
                "no keyboard LED sysfs nodes. hp::kbd_zoned_* is not exposed by this kernel".into(),
            );
        }
        for (zone, color, brightness) in frames {
            let targets: Vec<&LedNode> = if *zone == ZoneId::All {
                self.leds.iter().collect()
            } else {
                self.leds.iter().filter(|led| led.zone == *zone).collect()
            };
            if targets.is_empty() && *zone != ZoneId::All {
                return Err(format!("zone {} has no LED node", zone.as_str()));
            }
            for led in targets {
                write_led(&led.dir, *color, *brightness)?;
            }
        }
        Ok(())
    }
}

/// `Some(false)` when the backlight key has the lamp off.
/// `None` when this kernel build does not publish that state.
pub fn keyboard_lit() -> Option<bool> {
    let text = fs::read_to_string("/sys/devices/platform/omen-kbd/backlight").ok()?;
    match text.trim() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn discover_leds(directory: &Path) -> Vec<LedNode> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut leds = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        let keyboard = lower.contains("kbd") || lower.contains("keyboard");
        let hp = lower.contains("hp") || lower.contains("omen");
        if !(keyboard && hp) {
            continue;
        }
        let zone = if lower.contains("wasd") {
            ZoneId::Wasd
        } else if lower.contains("left") {
            ZoneId::Left
        } else if lower.contains("center") || lower.contains("middle") {
            ZoneId::Center
        } else if lower.contains("right") {
            ZoneId::Right
        } else {
            ZoneId::All
        };
        leds.push(LedNode {
            zone,
            dir: entry.path(),
        });
    }
    leds
}

fn find_hwmon(directory: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if read_trimmed(&dir.join("name")).as_deref() == Some(name)
            && (dir.join("pwm1").exists() || dir.join("pwm1_enable").exists())
        {
            return Some(dir);
        }
    }
    None
}

fn find_temp(directory: &Path, names: &[&str]) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        let Some(name) = read_trimmed(&dir.join("name")) else {
            continue;
        };
        if names.iter().any(|wanted| name == *wanted) {
            let temp = dir.join("temp1_input");
            if temp.exists() {
                return Some(temp);
            }
        }
    }
    None
}

fn write_led(dir: &Path, color: RgbColor, brightness: u8) -> Result<(), String> {
    let intensity = dir.join("multi_intensity");
    if intensity.exists() {
        write_sysfs(&intensity, &format!("{} {} {}", color.r, color.g, color.b))?;
    }
    let brightness_path = dir.join("brightness");
    if brightness_path.exists() {
        let max = read_u32(&dir.join("max_brightness")).unwrap_or(255).max(1);
        let scaled = u32::from(brightness) * max / 255;
        write_sysfs(&brightness_path, &scaled.to_string())?;
    } else if !intensity.exists() {
        return Err(format!(
            "{} has neither multi_intensity nor brightness",
            dir.display()
        ));
    }
    Ok(())
}

fn write_sysfs(path: &Path, value: &str) -> Result<(), String> {
    fs::write(path, format!("{value}\n")).map_err(|err| format!("write {}: {err}", path.display()))
}

fn can_write(path: &Path) -> bool {
    OpenOptions::new().write(true).open(path).is_ok()
}

fn existing(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn read_u32(path: &Path) -> Option<u32> {
    read_trimmed(path)?.parse().ok()
}

fn read_millicelsius(path: &Path) -> Option<u32> {
    let milli = read_trimmed(path)?.parse::<i32>().ok()?;
    if milli <= 0 {
        return None;
    }
    let celsius = u32::try_from(milli / 1000).ok()?;
    (celsius > 0 && celsius <= crate::MAX_CURVE_TEMP_C).then_some(celsius)
}

fn fmt_c(value: Option<u32>) -> String {
    value
        .map(|temp| format!("{temp}°C"))
        .unwrap_or_else(|| "n/a".into())
}

fn fmt_rpm(value: Option<u32>) -> String {
    value
        .map(|rpm| format!("{rpm} RPM"))
        .unwrap_or_else(|| "n/a".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_temp_uses_the_hotter_sensor() {
        let readings = Readings {
            cpu_c: Some(60),
            gpu_c: Some(72),
            fan1_rpm: None,
            fan2_rpm: None,
            pwm: None,
            pwm_enable: None,
            profile: None,
        };
        assert_eq!(readings.control_temp(), Some(72));
    }
}
