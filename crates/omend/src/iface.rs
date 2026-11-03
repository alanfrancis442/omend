// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use omen_common::Status;
use zbus::fdo;

use crate::engine::SharedEngine;

pub struct OmenIface {
    pub engine: SharedEngine,
}

#[zbus::interface(name = "dev.omend")]
impl OmenIface {
    fn get_temperatures(&self) -> fdo::Result<(u32, u32)> {
        Ok(self.lock()?.temperatures())
    }

    fn get_fan_speeds(&self) -> fdo::Result<(u32, u32)> {
        Ok(self.lock()?.fan_speeds())
    }

    fn set_fan_curve(&self, points: Vec<(u32, u32)>) -> fdo::Result<()> {
        self.lock()?.set_curve(points).map_err(fail)
    }

    fn save_fan_curve(
        &self,
        name: &str,
        points: Vec<(u32, u32)>,
        hysteresis: u32,
    ) -> fdo::Result<()> {
        self.lock()?
            .save_named_curve(name, points, hysteresis)
            .map_err(fail)
    }

    fn delete_fan_curve(&self, name: &str) -> fdo::Result<()> {
        self.lock()?.delete_named_curve(name).map_err(fail)
    }

    fn set_power_curve(&self, source: &str, name: &str) -> fdo::Result<()> {
        self.lock()?.set_power_curve(source, name).map_err(fail)
    }

    fn set_hysteresis(&self, secs: u32) -> fdo::Result<()> {
        self.lock()?.set_hysteresis(secs).map_err(fail)
    }

    fn set_preset(&self, name: &str) -> fdo::Result<()> {
        self.lock()?.set_preset(name).map_err(fail)
    }

    fn set_rgb_zone(&self, zone_id: u8, r: u8, g: u8, b: u8, brightness: u8) -> fdo::Result<()> {
        self.lock()?
            .set_rgb_zone(zone_id, r, g, b, brightness)
            .map_err(fail)
    }

    fn set_rgb_brightness(&self, brightness: u8) -> fdo::Result<()> {
        self.lock()?.set_rgb_brightness(brightness).map_err(fail)
    }

    fn set_rgb_effect(&self, effect: &str) -> fdo::Result<()> {
        self.lock()?.set_effect(effect).map_err(fail)
    }

    fn set_rgb_style(
        &self,
        speed: u8,
        depth: u8,
        saturation: u8,
        reverse: bool,
    ) -> fdo::Result<()> {
        self.lock()?
            .set_rgb_style(speed, depth, saturation, reverse)
            .map_err(fail)
    }

    fn set_audio_levels(&self, bass: u8, low: u8, mid: u8, high: u8) -> fdo::Result<()> {
        self.lock()?
            .set_audio_levels(bass, low, mid, high)
            .map_err(fail)
    }

    fn set_custom_effect(&self, motion: &str, steps: Vec<(u8, u8, u8)>) -> fdo::Result<()> {
        self.lock()?.set_custom_effect(motion, steps).map_err(fail)
    }

    fn set_platform_profile(&self, profile: &str) -> fdo::Result<()> {
        self.lock()?.set_profile(profile).map_err(fail)
    }

    fn reapply_state(&self) -> fdo::Result<()> {
        self.lock()?.reapply();
        Ok(())
    }

    fn get_status(&self) -> fdo::Result<String> {
        let status: Status = self.lock()?.status();
        serde_json::to_string(&status).map_err(|err| fail(err.to_string()))
    }
}

impl OmenIface {
    fn lock(&self) -> fdo::Result<std::sync::MutexGuard<'_, crate::engine::Engine>> {
        let engine = self.engine.lock().map_err(|err| fail(err.to_string()))?;
        if !engine.is_running() {
            return Err(fdo::Error::Failed(
                "daemon is starting or shutting down".into(),
            ));
        }
        Ok(engine)
    }
}

fn fail(message: String) -> fdo::Error {
    fdo::Error::Failed(message)
}
