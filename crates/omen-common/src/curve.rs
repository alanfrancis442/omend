// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::Preset;

pub const MAX_CURVE_POINTS: usize = 8;
pub const MAX_CURVE_TEMP_C: u32 = 140;
/// Manual curves cannot suppress cooling at or above this temperature.
pub const CRITICAL_TEMP_C: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CurveError {
    #[error("a fan curve needs at least two points")]
    TooFewPoints,
    #[error("a fan curve supports at most {MAX_CURVE_POINTS} points")]
    TooManyPoints,
    #[error("temperature {0} is outside 0-{MAX_CURVE_TEMP_C} degrees C")]
    TemperatureRange(u32),
    #[error("fan curve temperatures must increase")]
    Unsorted,
    #[error("pwm {0} is outside 0-255")]
    PwmRange(u32),
}

/// Temperature (°C) to PWM (0–255) map. Points are sorted by temperature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawCurve")]
pub struct Curve {
    points: Vec<(u32, u8)>,
}

#[derive(Deserialize)]
struct RawCurve {
    points: Vec<(u32, u32)>,
}

impl TryFrom<RawCurve> for Curve {
    type Error = CurveError;

    fn try_from(raw: RawCurve) -> Result<Self, Self::Error> {
        Self::new(raw.points)
    }
}

impl Curve {
    pub fn new(points: Vec<(u32, u32)>) -> Result<Self, CurveError> {
        if points.len() < 2 {
            return Err(CurveError::TooFewPoints);
        }
        if points.len() > MAX_CURVE_POINTS {
            return Err(CurveError::TooManyPoints);
        }
        let mut mapped = Vec::with_capacity(points.len());
        for (temp, pwm) in points {
            if temp > MAX_CURVE_TEMP_C {
                return Err(CurveError::TemperatureRange(temp));
            }
            if pwm > 255 {
                return Err(CurveError::PwmRange(pwm));
            }
            mapped.push((temp, pwm as u8));
        }
        for pair in mapped.windows(2) {
            if pair[1].0 <= pair[0].0 {
                return Err(CurveError::Unsorted);
            }
        }
        Ok(Self { points: mapped })
    }

    pub fn points(&self) -> &[(u32, u8)] {
        &self.points
    }

    pub fn as_dbus(&self) -> Vec<(u32, u32)> {
        self.points
            .iter()
            .map(|(temp, pwm)| (*temp, u32::from(*pwm)))
            .collect()
    }

    /// Linear interpolation between points.
    /// Below the first point the fans are off. At and above the last point the last PWM is held.
    pub fn pwm_at(&self, temp_c: u32) -> u8 {
        let points = &self.points;
        if temp_c < points[0].0 {
            return 0;
        }
        if temp_c >= points[points.len() - 1].0 {
            return points[points.len() - 1].1;
        }
        for window in points.windows(2) {
            let (t0, p0) = window[0];
            let (t1, p1) = window[1];
            if temp_c >= t0 && temp_c <= t1 {
                let span = i64::from(t1 - t0);
                let rise = i64::from(p1) - i64::from(p0);
                let value = i64::from(p0) + rise * i64::from(temp_c - t0) / span;
                return value.clamp(0, 255) as u8;
            }
        }
        points[points.len() - 1].1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanPlan {
    pub pwm: u8,
    /// `false` returns the fans to the EC's own curve.
    pub manual: bool,
    pub should_write: bool,
}

/// Curve selection, spin-down delay, and the EC watchdog re-write timer.
#[derive(Debug, Clone)]
pub struct ThermalControl {
    pub preset: Preset,
    pub custom: Curve,
    pub quiet: Curve,
    pub balanced: Curve,
    pub hysteresis: Duration,
    pub watchdog: Duration,
    held_pwm: u8,
    pending_drop: Option<(u8, Instant)>,
    last_write: Option<Instant>,
    applied_pwm: Option<u8>,
    applied_manual: Option<bool>,
}

impl ThermalControl {
    pub fn new(
        preset: Preset,
        custom: Curve,
        quiet: Curve,
        balanced: Curve,
        hysteresis: Duration,
        watchdog: Duration,
    ) -> Self {
        Self {
            preset,
            custom,
            quiet,
            balanced,
            hysteresis,
            watchdog,
            held_pwm: 0,
            pending_drop: None,
            last_write: None,
            applied_pwm: None,
            applied_manual: None,
        }
    }

    pub fn active_curve(&self) -> &Curve {
        match self.preset {
            Preset::Quiet => &self.quiet,
            Preset::Max | Preset::Auto => &self.balanced,
            Preset::Balanced => &self.balanced,
            Preset::Custom => &self.custom,
        }
    }

    /// Decide the PWM to hold for `temp_c` at time `now`.
    ///
    /// The hotter of the CPU and GPU is the input. Spin-ups apply immediately.
    /// Spin-downs wait out `hysteresis` so a short boost does not rev the fans.
    /// The EC watchdog is satisfied by `should_write` even when the target is unchanged.
    pub fn plan(&mut self, temp_c: u32, now: Instant) -> FanPlan {
        if self.preset == Preset::Auto {
            let should_write = self.applied_manual != Some(false)
                || self
                    .last_write
                    .is_none_or(|t| now.duration_since(t) >= self.watchdog);
            return FanPlan {
                pwm: 0,
                manual: false,
                should_write,
            };
        }

        let desired = if self.preset == Preset::Max || temp_c >= CRITICAL_TEMP_C {
            255
        } else {
            self.active_curve().pwm_at(temp_c)
        };
        let pwm = self.apply_hysteresis(desired, now);
        // hp-wmi replaces a manual PWM of 0 with the BIOS minimum, so the fans
        // keep spinning. pwm1_enable=2 is what actually lets them stop.
        let manual = pwm > 0;
        let changed = self.applied_pwm != Some(pwm) || self.applied_manual != Some(manual);
        let due = self
            .last_write
            .is_none_or(|t| now.duration_since(t) >= self.watchdog);
        FanPlan {
            pwm,
            manual,
            should_write: changed || due,
        }
    }

    pub fn mark_written(&mut self, plan: FanPlan, now: Instant) {
        self.applied_pwm = Some(plan.pwm);
        self.applied_manual = Some(plan.manual);
        self.last_write = Some(now);
        if plan.manual {
            self.held_pwm = plan.pwm;
        }
    }

    /// Force the next `plan` to write, used after suspend.
    pub fn invalidate(&mut self) {
        self.last_write = None;
        self.applied_pwm = None;
        self.applied_manual = None;
    }

    fn apply_hysteresis(&mut self, desired: u8, now: Instant) -> u8 {
        if desired >= self.held_pwm {
            self.pending_drop = None;
            self.held_pwm = desired;
            return desired;
        }
        match self.pending_drop {
            Some((pwm, since))
                if pwm == desired && now.duration_since(since) >= self.hysteresis =>
            {
                self.pending_drop = None;
                self.held_pwm = desired;
                desired
            }
            Some((pwm, _)) if pwm == desired => self.held_pwm,
            _ => {
                self.pending_drop = Some((desired, now));
                self.held_pwm
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unbounded_inputs_and_deserialization_cannot_bypass_validation() {
        assert_eq!(Curve::new(vec![(0, 0); 9]), Err(CurveError::TooManyPoints));
        assert_eq!(
            Curve::new(vec![(0, 0), (u32::MAX, 255)]),
            Err(CurveError::TemperatureRange(u32::MAX))
        );
        for text in [
            r#"{"points":[]}"#,
            r#"{"points":[[80,10],[40,20]]}"#,
            r#"{"points":[[0,0],[4294967295,255]]}"#,
        ] {
            assert!(serde_json::from_str::<Curve>(text).is_err());
        }
        let curve = Curve::new(vec![(0, 255), (140, 0)]).unwrap();
        assert_eq!(curve.pwm_at(70), 128);
        assert_eq!(curve.pwm_at(u32::MAX), 0);
        assert_eq!(
            serde_json::from_str::<Curve>(&serde_json::to_string(&curve).unwrap()).unwrap(),
            curve
        );
    }

    #[test]
    fn critical_temperature_overrides_manual_curves_immediately_but_preserves_auto() {
        for preset in [
            Preset::Quiet,
            Preset::Balanced,
            Preset::Custom,
            Preset::Max,
            Preset::Auto,
        ] {
            let curve = Curve::new(vec![(0, 1), (140, 1)]).unwrap();
            let mut thermal = ThermalControl::new(
                preset,
                curve.clone(),
                curve.clone(),
                curve,
                Duration::from_secs(30),
                Duration::from_secs(30),
            );
            let now = Instant::now();
            let first = thermal.plan(50, now);
            thermal.mark_written(first, now);
            let emergency = thermal.plan(CRITICAL_TEMP_C, now + Duration::from_millis(1));
            assert_eq!(emergency.manual, preset != Preset::Auto);
            assert_eq!(emergency.pwm, if preset == Preset::Auto { 0 } else { 255 });
        }
    }

    fn sample() -> Curve {
        Curve::new(vec![(40, 0), (60, 100), (80, 200)]).unwrap()
    }

    #[test]
    fn interpolates_and_clamps() {
        let curve = sample();
        assert_eq!(curve.pwm_at(30), 0);
        assert_eq!(curve.pwm_at(40), 0);
        assert_eq!(curve.pwm_at(50), 50);
        assert_eq!(curve.pwm_at(70), 150);
        assert_eq!(curve.pwm_at(95), 200);
    }

    #[test]
    fn below_the_first_point_is_off_even_when_that_point_is_not() {
        let curve = Curve::new(vec![(52, 51), (90, 255)]).unwrap();
        assert_eq!(curve.pwm_at(41), 0);
        assert_eq!(curve.pwm_at(51), 0);
        assert_eq!(curve.pwm_at(52), 51);
    }

    #[test]
    fn rejects_unsorted_and_empty_curves() {
        assert!(matches!(
            Curve::new(vec![(10, 1)]),
            Err(CurveError::TooFewPoints)
        ));
        assert!(matches!(
            Curve::new(vec![(80, 10), (40, 20)]),
            Err(CurveError::Unsorted)
        ));
        assert!(matches!(
            Curve::new(vec![(40, 0), (80, 300)]),
            Err(CurveError::PwmRange(300))
        ));
    }

    #[test]
    fn hysteresis_delays_spindown_but_not_spinup() {
        let curve = Curve::new(vec![(0, 0), (100, 100)]).unwrap();
        let mut thermal = ThermalControl::new(
            Preset::Custom,
            curve.clone(),
            curve.clone(),
            curve,
            Duration::from_secs(4),
            Duration::from_secs(15),
        );
        let t0 = Instant::now();
        let up = thermal.plan(80, t0);
        assert_eq!(up.pwm, 80);
        assert!(up.should_write);
        thermal.mark_written(up, t0);

        let held = thermal.plan(40, t0 + Duration::from_secs(1));
        assert_eq!(held.pwm, 80);
        assert!(!held.should_write);

        let dropped = thermal.plan(40, t0 + Duration::from_secs(5));
        assert_eq!(dropped.pwm, 40);
        assert!(dropped.should_write);
    }

    #[test]
    fn watchdog_rewrites_same_target() {
        let curve = Curve::new(vec![(0, 40), (100, 40)]).unwrap();
        let mut thermal = ThermalControl::new(
            Preset::Custom,
            curve.clone(),
            curve.clone(),
            curve,
            Duration::from_secs(3),
            Duration::from_secs(10),
        );
        let t0 = Instant::now();
        let first = thermal.plan(50, t0);
        thermal.mark_written(first, t0);
        let quiet = thermal.plan(50, t0 + Duration::from_secs(3));
        assert!(!quiet.should_write);
        let again = thermal.plan(50, t0 + Duration::from_secs(10));
        assert!(again.should_write);
        assert_eq!(again.pwm, 40);
    }

    #[test]
    fn below_the_curve_hands_the_fans_back_to_firmware() {
        let curve = Curve::new(vec![(52, 51), (90, 255)]).unwrap();
        let mut thermal = ThermalControl::new(
            Preset::Custom,
            curve.clone(),
            curve.clone(),
            curve,
            Duration::from_secs(4),
            Duration::from_secs(15),
        );
        let t0 = Instant::now();
        let running = thermal.plan(60, t0);
        assert!(running.manual);
        assert!(running.pwm > 0);
        thermal.mark_written(running, t0);

        let held = thermal.plan(41, t0 + Duration::from_secs(1));
        assert!(held.manual);
        assert_eq!(held.pwm, running.pwm);

        let stopped = thermal.plan(41, t0 + Duration::from_secs(5));
        assert_eq!(stopped.pwm, 0);
        assert!(!stopped.manual);
        assert!(stopped.should_write);
    }

    #[test]
    fn max_preset_is_full_pwm() {
        let curve = Curve::new(vec![(0, 10), (100, 10)]).unwrap();
        let mut thermal = ThermalControl::new(
            Preset::Max,
            curve.clone(),
            curve.clone(),
            curve,
            Duration::from_secs(3),
            Duration::from_secs(30),
        );
        let plan = thermal.plan(30, Instant::now());
        assert_eq!(plan.pwm, 255);
        assert!(plan.manual);
    }
}
