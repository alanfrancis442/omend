// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared contract for the omend daemon, the CLI, and the GUI.
//!
//! The privileged daemon, the CLI, and the GUI all speak this crate: sensor
//! discovery, fan-curve math, the on-disk config, and the D-Bus proxy.

mod config;
mod curve;
mod effects;
mod power;
mod sensors;
mod types;

pub use config::Config;
pub use curve::{
    CRITICAL_TEMP_C, Curve, CurveError, FanPlan, MAX_CURVE_POINTS, MAX_CURVE_TEMP_C, ThermalControl,
};
pub use effects::{audio_frame, effect_frame, hsv_to_rgb, rgb_to_hsv};
pub use power::mains_online;
pub use sensors::{HardwareReport, Readings, Sensors, keyboard_lit};
pub use types::{
    CustomMotion, CustomSequence, Effect, EffectStyle, Preset, Profile, RgbColor, SERVICE_NAME,
    SERVICE_PATH, SavedCurveInfo, Status, ZoneId, parse_power_source,
};

pub const INTERFACE: &str = "dev.omend";

#[zbus::proxy(
    interface = "dev.omend",
    default_service = "dev.omend",
    default_path = "/dev/omend"
)]
pub trait Omend {
    fn get_temperatures(&self) -> zbus::Result<(u32, u32)>;
    fn get_fan_speeds(&self) -> zbus::Result<(u32, u32)>;
    fn set_fan_curve(&self, points: Vec<(u32, u32)>) -> zbus::Result<()>;
    fn save_fan_curve(
        &self,
        name: &str,
        points: Vec<(u32, u32)>,
        hysteresis: u32,
    ) -> zbus::Result<()>;
    fn delete_fan_curve(&self, name: &str) -> zbus::Result<()>;
    /// `source` is `ac` or `battery`. An empty `name` leaves that source on manual control.
    fn set_power_curve(&self, source: &str, name: &str) -> zbus::Result<()>;
    fn set_hysteresis(&self, secs: u32) -> zbus::Result<()>;
    fn set_preset(&self, name: &str) -> zbus::Result<()>;
    /// Update a zone's base color without selecting a different lighting effect.
    fn set_rgb_zone(&self, zone_id: u8, r: u8, g: u8, b: u8, brightness: u8) -> zbus::Result<()>;
    fn set_rgb_brightness(&self, brightness: u8) -> zbus::Result<()>;
    fn set_rgb_effect(&self, effect: &str) -> zbus::Result<()>;
    fn set_rgb_style(
        &self,
        speed: u8,
        depth: u8,
        saturation: u8,
        reverse: bool,
    ) -> zbus::Result<()>;
    fn set_audio_levels(&self, bass: u8, low: u8, mid: u8, high: u8) -> zbus::Result<()>;
    fn set_custom_effect(&self, motion: &str, steps: Vec<(u8, u8, u8)>) -> zbus::Result<()>;
    fn set_platform_profile(&self, profile: &str) -> zbus::Result<()>;
    fn reapply_state(&self) -> zbus::Result<()>;
    fn get_status(&self) -> zbus::Result<String>;
}

/// Connect to a running `omend`.
///
/// A user CLI prefers the system bus, where the root daemon lives, and falls
/// back to the session bus used by `omend --session`.
pub async fn connect(session_only: bool) -> zbus::Result<zbus::Connection> {
    if session_only {
        return zbus::Connection::session().await;
    }
    if let Ok(conn) = zbus::Connection::system().await {
        let proxy = OmendProxy::new(&conn).await?;
        if proxy.get_status().await.is_ok() {
            return Ok(conn);
        }
    }
    zbus::Connection::session().await
}

pub fn waybar_json(status: &Status) -> String {
    let rpm = status.fan1_rpm.max(status.fan2_rpm);
    let text = if status.daemon_ok {
        format!("󰈐 {rpm} RPM")
    } else {
        "󰈐 offline".to_string()
    };
    let tooltip = format!(
        "CPU: {}°C | GPU: {}°C\nMode: {}\nProfile: {}",
        status.cpu_c, status.gpu_c, status.preset, status.profile
    );
    serde_json::json!({
        "text": text,
        "tooltip": tooltip,
        "class": status.waybar_class(),
    })
    .to_string()
}
