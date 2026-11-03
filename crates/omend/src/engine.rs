// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::Mutex;
use std::time::Instant;

use omen_common::{
    Config, Curve, CustomMotion, CustomSequence, Effect, EffectStyle, Preset, Profile, RgbColor,
    SavedCurveInfo, Sensors, Status, ThermalControl, ZoneId, audio_frame, effect_frame,
    keyboard_lit, mains_online, parse_power_source,
};

pub struct Engine {
    running: bool,
    config: Config,
    config_path: Option<std::path::PathBuf>,
    sensors: Sensors,
    thermal: ThermalControl,
    tick: u64,
    rgb_frame: u64,
    last_error: String,
    persistence_error: String,
    /// Last mains state we applied a power curve for. `None` until the first reading.
    last_mains: Option<bool>,
    /// Bass, low, mid, high. Live input for the audio effect.
    audio_levels: [u8; 4],
    audio_updated: Option<Instant>,
    /// Last LED write caused by an audio packet. Bounds sysfs writes per caller.
    audio_written: Option<Instant>,
    /// Last backlight-key reading. A rising edge writes the current colors once.
    rgb_lit: bool,
}

impl Engine {
    pub fn new(config: Config, config_path: Option<std::path::PathBuf>, sensors: Sensors) -> Self {
        let sensors =
            sensors.with_emergency_max_recovery(config.thermal.experimental_emergency_max);
        if config.thermal.experimental_emergency_max {
            tracing::warn!(
                "experimental maximum-fan recovery enabled; physical behavior is unvalidated"
            );
        }
        let thermal = ThermalControl::new(
            config.thermal.preset,
            curve_or_default(config.custom_curve(), Config::default().custom_curve()),
            curve_or_default(config.quiet_curve(), Config::default().quiet_curve()),
            curve_or_default(config.balanced_curve(), Config::default().balanced_curve()),
            config.hysteresis(),
            config.watchdog(),
        );
        Self {
            running: false,
            config,
            config_path,
            sensors,
            thermal,
            tick: 0,
            rgb_frame: 0,
            last_error: String::new(),
            persistence_error: String::new(),
            last_mains: None,
            audio_levels: [0; 4],
            audio_updated: None,
            audio_written: None,
            rgb_lit: true,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Called only after exclusive process and D-Bus ownership are established.
    pub fn start(&mut self) {
        self.running = true;
        self.reapply();
    }

    /// Do not persist the recovery mode: preserve the user's selected preset.
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.running = false;
        self.thermal.invalidate();
        if self.sensors.fan_recovery_available() {
            self.sensors.recover_fans()?;
        }
        Ok(())
    }

    pub fn poll(&mut self) {
        if !self.running {
            return;
        }
        self.follow_power();
        self.tick = self.tick.wrapping_add(1);
        let readings = self.sensors.read();
        let Some(temperature) = self.sensors.control_temp(&readings) else {
            self.thermal.invalidate();
            if self.sensors.fan_recovery_available() {
                let recovery_name = self.sensors.fan_recovery_name();
                self.last_error = match self.sensors.recover_fans() {
                    Ok(()) => format!(
                        "fan: temperature sensor unavailable; requested {recovery_name}; cooling unverified"
                    ),
                    Err(error) => format!(
                        "fan: temperature sensor unavailable; {recovery_name} recovery request failed: {error}"
                    ),
                };
            }
            return;
        };
        let plan = self.thermal.plan(temperature, Instant::now());
        // A mode reset must not wait for the periodic rewrite. Do not compare
        // PWM: hp-wmi can report measured RPM scaled to PWM, not requested duty.
        // Acceleration, quantization and firmware limits would look like faults.
        let readback_matches = readings.pwm_enable == Some(if plan.manual { 1 } else { 2 });
        if (plan.should_write || !readback_matches) && self.sensors.fan_writable {
            match self.sensors.apply_fan(plan.manual, plan.pwm) {
                Ok(()) => {
                    self.thermal.mark_written(plan, Instant::now());
                    if self.last_error.starts_with("fan:") {
                        self.last_error.clear();
                    }
                }
                Err(err) => {
                    // apply_fan may have requested recovery after a partial
                    // write. Retry next poll even if the target is unchanged.
                    self.thermal.invalidate();
                    self.last_error = format!("fan: {err}");
                    tracing::warn!("{err}");
                }
            }
        }

        if self.config.rgb.effect == Effect::Static && self.tick == 1 {
            let _ = self.write_rgb();
        }
    }

    /// Advance a software lighting effect. The fan poll stays on its own interval.
    /// The backlight key stops the frame clock and every LED write until it comes back on.
    pub fn animate(&mut self) {
        if !self.running {
            return;
        }
        if !self.sensors.rgb_available() {
            return;
        }
        match keyboard_lit() {
            Some(false) => {
                self.rgb_lit = false;
                return;
            }
            Some(true) => {
                let resume = !self.rgb_lit;
                self.rgb_lit = true;
                if resume && let Err(err) = self.write_rgb() {
                    self.last_error = format!("rgb: {err}");
                }
                if self.config.rgb.effect == Effect::Static {
                    return;
                }
            }
            None => {
                if self.config.rgb.effect == Effect::Static {
                    return;
                }
            }
        }
        if self.config.rgb.effect == Effect::Audio {
            // Audio packets already write their frame. Only write here when
            // a lost/closed GUI leaves a stale spectrum to fade to silence.
            if live_audio(self.audio_levels, self.audio_updated, Instant::now())
                != self.audio_levels
            {
                self.audio_levels = [0; 4];
                self.audio_updated = None;
                if let Err(err) = self.write_rgb() {
                    self.last_error = format!("rgb: {err}");
                }
            }
            return;
        }
        self.rgb_frame = self.rgb_frame.wrapping_add(1);
        if let Err(err) = self.write_rgb() {
            self.last_error = format!("rgb: {err}");
        }
    }

    pub fn reapply(&mut self) {
        if !self.running {
            return;
        }
        self.thermal.invalidate();
        self.poll();
        if self.sensors.profile_writable()
            && let Err(err) = self.sensors.apply_profile(self.config.profile.as_str())
        {
            self.last_error = format!("profile: {err}");
        }
        if let Err(err) = self.write_rgb()
            && self.sensors.rgb_available()
        {
            self.last_error = format!("rgb: {err}");
        }
    }

    pub fn set_preset(&mut self, name: &str) -> Result<(), String> {
        let preset = Preset::parse(name).ok_or_else(|| format!("unknown fan preset '{name}'"))?;
        self.config.thermal.preset = preset;
        self.thermal.preset = preset;
        self.thermal.invalidate();
        self.poll();
        let applied = self.fan_result();
        self.finish_change(applied)
    }

    pub fn set_curve(&mut self, points: Vec<(u32, u32)>) -> Result<(), String> {
        let curve = Curve::new(points).map_err(|err| err.to_string())?;
        let hysteresis = self.config.thermal.hysteresis_secs;
        self.remember_assigned(&curve, hysteresis)?;
        self.apply_custom(curve, hysteresis);
        self.poll();
        let applied = self.fan_result();
        self.finish_change(applied)
    }

    pub fn save_named_curve(
        &mut self,
        name: &str,
        points: Vec<(u32, u32)>,
        hysteresis: u32,
    ) -> Result<(), String> {
        let curve = Curve::new(points).map_err(|err| err.to_string())?;
        let hysteresis = u64::from(hysteresis.clamp(1, 30));
        self.config.upsert_saved(name, &curve, hysteresis)?;
        self.apply_custom(curve, hysteresis);
        self.poll();
        let applied = self.fan_result();
        self.finish_change(applied)
    }

    pub fn delete_named_curve(&mut self, name: &str) -> Result<(), String> {
        if !self.config.delete_saved(name) {
            return Err(format!("no saved curve '{name}'"));
        }
        self.persist()
    }

    /// `source` is `ac` or `battery`. An empty name leaves that source on manual control.
    pub fn set_power_curve(&mut self, source: &str, name: &str) -> Result<(), String> {
        let on_ac =
            parse_power_source(source).ok_or_else(|| format!("unknown power source '{source}'"))?;
        let resolved = self.config.resolve_power_name(name)?;
        if on_ac {
            self.config.thermal.plugged_in = resolved.clone();
        } else {
            self.config.thermal.on_battery = resolved.clone();
        }
        let applied = if mains_online() == Some(on_ac) && !resolved.is_empty() {
            self.activate_named(&resolved).and_then(|()| {
                self.last_mains = Some(on_ac);
                self.poll();
                self.fan_result()
            })
        } else {
            Ok(())
        };
        self.finish_change(applied)
    }

    pub fn set_hysteresis(&mut self, secs: u32) -> Result<(), String> {
        let secs = u64::from(secs.clamp(1, 30));
        self.config.thermal.hysteresis_secs = secs;
        self.thermal.hysteresis = std::time::Duration::from_secs(secs);
        if let Ok(curve) = self.config.custom_curve() {
            self.remember_assigned(&curve, secs)?;
        }
        self.persist()
    }

    pub fn set_profile(&mut self, name: &str) -> Result<(), String> {
        let profile = if name == "next" {
            let current = self
                .sensors
                .read()
                .profile
                .as_deref()
                .and_then(Profile::parse)
                .unwrap_or(self.config.profile);
            current.next()
        } else {
            Profile::parse(name).ok_or_else(|| format!("unknown platform profile '{name}'"))?
        };
        self.sensors.apply_profile(profile.as_str())?;
        self.config.profile = profile;
        self.persist()
    }

    pub fn set_rgb_zone(
        &mut self,
        zone_id: u8,
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
    ) -> Result<(), String> {
        let zone = ZoneId::from_u8(zone_id).ok_or_else(|| format!("unknown zone id {zone_id}"))?;
        self.config.set_zone_color(zone, RgbColor::new(r, g, b));
        self.config.rgb.brightness = brightness;
        let applied = self.write_rgb();
        self.finish_change(applied)
    }

    /// Brightness applies to whatever effect is already running.
    pub fn set_rgb_brightness(&mut self, brightness: u8) -> Result<(), String> {
        self.config.rgb.brightness = brightness;
        let applied = self.write_rgb();
        self.finish_change(applied)
    }

    pub fn set_effect(&mut self, name: &str) -> Result<(), String> {
        let effect = Effect::parse(name).ok_or_else(|| format!("unknown rgb effect '{name}'"))?;
        self.config.rgb.effect = effect;
        let applied = self.write_rgb();
        self.finish_change(applied)
    }

    pub fn set_custom_effect(
        &mut self,
        motion: &str,
        steps: Vec<(u8, u8, u8)>,
    ) -> Result<(), String> {
        let motion = CustomMotion::parse(motion)
            .ok_or_else(|| format!("unknown custom motion '{motion}'"))?;
        if !(2..=8).contains(&steps.len()) {
            return Err("a custom effect needs 2 to 8 colors".into());
        }
        self.config.rgb.custom_motion = motion;
        self.config.rgb.custom_steps = steps
            .into_iter()
            .map(|(r, g, b)| RgbColor::new(r, g, b))
            .collect();
        self.config.rgb.effect = Effect::Custom;
        let applied = self.write_rgb();
        self.finish_change(applied)
    }

    pub fn set_rgb_style(
        &mut self,
        speed: u8,
        depth: u8,
        saturation: u8,
        reverse: bool,
    ) -> Result<(), String> {
        let style = EffectStyle {
            speed,
            depth,
            saturation,
            reverse,
        }
        .clamped();
        self.config.rgb.speed = style.speed;
        self.config.rgb.depth = style.depth;
        self.config.rgb.saturation = style.saturation;
        self.config.rgb.reverse = style.reverse;
        let applied = self.write_rgb();
        self.finish_change(applied)
    }

    pub fn set_audio_levels(&mut self, bass: u8, low: u8, mid: u8, high: u8) -> Result<(), String> {
        let now = Instant::now();
        self.audio_levels = [bass, low, mid, high];
        self.audio_updated = Some(now);
        if self.config.rgb.effect != Effect::Audio || !audio_write_due(self.audio_written, now) {
            // A flood of packets cannot turn into unbounded sysfs writes while
            // holding the engine lock. The next packet shows the latest levels.
            return Ok(());
        }
        self.audio_written = Some(now);
        self.write_rgb()
    }

    pub fn temperatures(&self) -> (u32, u32) {
        let readings = self.sensors.read();
        (readings.cpu_c.unwrap_or(0), readings.gpu_c.unwrap_or(0))
    }

    pub fn fan_speeds(&self) -> (u32, u32) {
        let readings = self.sensors.read();
        (
            readings.fan1_rpm.unwrap_or(0),
            readings.fan2_rpm.unwrap_or(0),
        )
    }

    pub fn status(&self) -> Status {
        let readings = self.sensors.read();
        let mut notes = Vec::new();
        if self.sensors.control_temp(&readings).is_none() {
            notes.push("temperature sensor unavailable; manual fan curves are suspended".into());
        }
        if !self.last_error.is_empty() {
            notes.push(self.last_error.clone());
        }
        if !self.persistence_error.is_empty() {
            notes.push(self.persistence_error.clone());
        }
        if !self.sensors.fan_writable {
            notes.push("fan control is not writable; start omend as root".into());
        }
        if !self.sensors.profile_writable() {
            notes.push("platform profile is not writable from this process".into());
        }
        if !self.sensors.rgb_available() {
            notes.push("keyboard LED sysfs is absent on this kernel".into());
        }
        let message = notes.join(" · ");
        Status {
            daemon_ok: true,
            cpu_c: readings.cpu_c.unwrap_or(0),
            gpu_c: readings.gpu_c.unwrap_or(0),
            fan1_rpm: readings.fan1_rpm.unwrap_or(0),
            fan2_rpm: readings.fan2_rpm.unwrap_or(0),
            pwm: readings.pwm.unwrap_or(0),
            preset: self.config.thermal.preset.to_string(),
            profile: readings
                .profile
                .unwrap_or_else(|| self.config.profile.to_string()),
            effect: self.config.rgb.effect.to_string(),
            rgb_speed: self.config.rgb.speed,
            rgb_depth: self.config.rgb.depth,
            rgb_saturation: self.config.rgb.saturation,
            rgb_reverse: self.config.rgb.reverse,
            custom_motion: self.config.rgb.custom_motion.as_str().into(),
            custom_steps: self
                .config
                .rgb
                .custom_steps
                .iter()
                .map(|color| (color.r, color.g, color.b))
                .collect(),
            rgb_available: self.sensors.rgb_available(),
            fan_control_available: self.sensors.fan_writable,
            message,
            curve: self.thermal.active_curve().as_dbus(),
            hysteresis_secs: u32::try_from(self.config.thermal.hysteresis_secs).unwrap_or(4),
            saved: self
                .config
                .thermal
                .saved
                .iter()
                .map(|saved| SavedCurveInfo {
                    name: saved.name.clone(),
                    hysteresis_secs: u32::try_from(saved.hysteresis_secs).unwrap_or(4),
                    points: saved
                        .points
                        .iter()
                        .map(|point| (point.temp, point.pwm))
                        .collect(),
                })
                .collect(),
            plugged_in: self.config.thermal.plugged_in.clone(),
            on_battery: self.config.thermal.on_battery.clone(),
            power: match mains_online() {
                Some(true) => "ac",
                Some(false) => "battery",
                None => "unknown",
            }
            .into(),
            library: true,
            rgb_on: keyboard_lit().unwrap_or(true),
        }
    }

    fn follow_power(&mut self) {
        let Some(on_ac) = mains_online() else {
            return;
        };
        if self.last_mains == Some(on_ac) {
            return;
        }
        self.last_mains = Some(on_ac);
        let choice = if on_ac {
            self.config.thermal.plugged_in.clone()
        } else {
            self.config.thermal.on_battery.clone()
        };
        if choice.is_empty() {
            return;
        }
        if let Err(err) = self.activate_named(&choice) {
            self.last_error = err;
            return;
        }
        // persist() keeps its own warning, independent of transient hardware
        // errors, so the next successful save can clear it completely.
        let _ = self.persist();
    }

    fn activate_named(&mut self, name: &str) -> Result<(), String> {
        if let Some(saved) = self.config.saved_curve(name).cloned() {
            let points = saved
                .points
                .iter()
                .map(|point| (point.temp, point.pwm))
                .collect();
            let curve = Curve::new(points).map_err(|err| err.to_string())?;
            let hysteresis = saved.hysteresis_secs.clamp(1, 30);
            self.apply_custom(curve, hysteresis);
            return Ok(());
        }
        let preset = Preset::parse(name).ok_or_else(|| format!("unknown fan setting '{name}'"))?;
        if preset == Preset::Custom {
            return Err("pick a saved curve, or quiet, balanced, max, or auto".into());
        }
        self.config.thermal.preset = preset;
        self.thermal.preset = preset;
        self.thermal.invalidate();
        Ok(())
    }

    /// Keep the saved curve that belongs to the current cable state in step with Apply.
    fn remember_assigned(&mut self, curve: &Curve, hysteresis_secs: u64) -> Result<(), String> {
        let Some(on_ac) = mains_online() else {
            return Ok(());
        };
        let assigned = if on_ac {
            self.config.thermal.plugged_in.clone()
        } else {
            self.config.thermal.on_battery.clone()
        };
        if self.config.saved_curve(&assigned).is_some() {
            self.config
                .upsert_saved(&assigned, curve, hysteresis_secs)?;
        }
        Ok(())
    }

    fn apply_custom(&mut self, curve: Curve, hysteresis_secs: u64) {
        self.config.set_custom_curve(&curve);
        self.config.thermal.preset = Preset::Custom;
        self.config.thermal.hysteresis_secs = hysteresis_secs;
        self.thermal.custom = curve;
        self.thermal.preset = Preset::Custom;
        self.thermal.hysteresis = std::time::Duration::from_secs(hysteresis_secs);
        self.thermal.invalidate();
    }

    fn write_rgb(&mut self) -> Result<(), String> {
        if keyboard_lit() == Some(false) {
            self.rgb_lit = false;
            return Ok(());
        }
        if !self.sensors.rgb_available() {
            return Err(
                "no keyboard LED sysfs nodes. hp::kbd_zoned_* is not exposed by this kernel".into(),
            );
        }
        let colors = [ZoneId::Left, ZoneId::Wasd, ZoneId::Center, ZoneId::Right].map(|zone| {
            (
                zone,
                self.config
                    .zone_color(zone)
                    .expect("a named keyboard zone stores one color"),
            )
        });
        let style = EffectStyle {
            speed: self.config.rgb.speed,
            depth: self.config.rgb.depth,
            saturation: self.config.rgb.saturation,
            reverse: self.config.rgb.reverse,
        };
        let custom = CustomSequence {
            motion: self.config.rgb.custom_motion,
            steps: self.config.rgb.custom_steps.clone(),
        };
        let frames = if self.config.rgb.effect == Effect::Audio {
            audio_frame(
                &colors,
                &live_audio(self.audio_levels, self.audio_updated, Instant::now()),
                self.config.rgb.brightness,
                style,
            )
        } else {
            effect_frame(
                self.config.rgb.effect,
                &colors,
                self.rgb_frame,
                self.config.rgb.brightness,
                style,
                &custom,
            )
        };
        self.sensors.apply_rgb(&frames)
    }

    fn fan_result(&self) -> Result<(), String> {
        if !self.sensors.fan_writable {
            return Err("fan control is not writable (omend must run as root)".into());
        }
        if self.last_error.starts_with("fan:") {
            return Err(self.last_error.clone());
        }
        Ok(())
    }

    /// Runtime state and hardware may already have changed when saving fails.
    /// Never acknowledge such a request as durably saved, and preserve both
    /// errors when the hardware operation also failed.
    fn finish_change(&mut self, applied: Result<(), String>) -> Result<(), String> {
        let saved = self.persist();
        match (applied, saved) {
            (Ok(()), result) => result,
            (Err(hardware), Ok(())) => Err(hardware),
            (Err(hardware), Err(save)) => Err(format!("{hardware}; {save}")),
        }
    }

    fn persist(&mut self) -> Result<(), String> {
        let result = self
            .config_path
            .as_ref()
            .ok_or_else(|| {
                "no configuration destination; start with --config to save settings".to_string()
            })
            .and_then(|path| self.config.save(path));
        match result {
            Ok(()) => {
                self.persistence_error.clear();
                Ok(())
            }
            Err(error) => {
                let message = format!(
                    "settings are not confirmed saved; runtime state or hardware may already have changed: {error}"
                );
                self.persistence_error = message.clone();
                tracing::warn!("{message}");
                Err(message)
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if self.running
            && let Err(error) = self.shutdown()
        {
            tracing::error!("fan recovery request failed: {error}");
        }
    }
}

fn curve_or_default(
    parsed: Result<Curve, omen_common::CurveError>,
    fallback: Result<Curve, omen_common::CurveError>,
) -> Curve {
    parsed.unwrap_or_else(|err| {
        tracing::warn!("ignoring invalid curve ({err})");
        fallback.expect("built-in curve")
    })
}

pub type SharedEngine = std::sync::Arc<Mutex<Engine>>;

/// At most one audio-driven LED frame per interval (about 66 frames a second).
const AUDIO_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(15);

fn audio_write_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|at| now.saturating_duration_since(at) >= AUDIO_MIN_INTERVAL)
}

fn live_audio(levels: [u8; 4], updated: Option<Instant>, now: Instant) -> [u8; 4] {
    if updated
        .is_some_and(|at| now.saturating_duration_since(at) < std::time::Duration::from_millis(400))
    {
        levels
    } else {
        [0; 4]
    }
}

#[cfg(test)]
mod audio_tests {
    use super::*;

    #[test]
    fn a_disconnected_visualizer_cannot_leave_a_stale_bright_frame() {
        let now = Instant::now();
        assert_eq!(
            live_audio([255, 128, 64, 32], Some(now), now),
            [255, 128, 64, 32]
        );
        assert_eq!(
            live_audio(
                [255; 4],
                Some(now - std::time::Duration::from_millis(401)),
                now
            ),
            [0; 4]
        );
        assert_eq!(live_audio([255; 4], None, now), [0; 4]);
    }

    #[test]
    fn audio_packet_floods_are_rate_limited() {
        let now = Instant::now();
        assert!(audio_write_due(None, now));
        assert!(!audio_write_due(Some(now), now));
        assert!(!audio_write_due(
            Some(now),
            now + std::time::Duration::from_millis(14)
        ));
        assert!(audio_write_due(Some(now), now + AUDIO_MIN_INTERVAL));
    }
}
