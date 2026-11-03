// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::{Deserialize, Serialize};

pub const SERVICE_NAME: &str = "dev.omend";
pub const SERVICE_PATH: &str = "/dev/omend";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Quiet,
    Balanced,
    Max,
    Custom,
    /// Hand the fans back to the embedded controller.
    Auto,
}

impl Preset {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "quiet" => Some(Self::Quiet),
            "balanced" | "balance" => Some(Self::Balanced),
            "max" | "turbo" => Some(Self::Max),
            "custom" => Some(Self::Custom),
            "auto" | "bios" => Some(Self::Auto),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Balanced => "balanced",
            Self::Max => "max",
            Self::Custom => "custom",
            Self::Auto => "auto",
        }
    }
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `Some(true)` is wall power, `Some(false)` is battery.
pub fn parse_power_source(name: &str) -> Option<bool> {
    match name.trim().to_ascii_lowercase().as_str() {
        "ac" | "plugged" | "mains" => Some(true),
        "battery" | "unplugged" => Some(false),
        _ => None,
    }
}

/// ACPI platform profile names exposed by hp-wmi on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    LowPower,
    Balanced,
    Performance,
}

impl Profile {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "low-power" | "quiet" | "low" => Some(Self::LowPower),
            "balanced" | "balance" => Some(Self::Balanced),
            "performance" | "perf" | "max" | "turbo" => Some(Self::Performance),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LowPower => "low-power",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::LowPower => Self::Balanced,
            Self::Balanced => Self::Performance,
            Self::Performance => Self::LowPower,
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneId {
    All = 0,
    Wasd = 1,
    Left = 2,
    Center = 3,
    Right = 4,
}

impl ZoneId {
    pub fn from_u8(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::All),
            1 => Some(Self::Wasd),
            2 => Some(Self::Left),
            3 => Some(Self::Center),
            4 => Some(Self::Right),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Wasd => "wasd",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "all" | "single" | "0" => Some(Self::All),
            "wasd" | "1" => Some(Self::Wasd),
            "left" | "2" => Some(Self::Left),
            "center" | "3" => Some(Self::Center),
            "right" | "4" => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Static,
    Breathing,
    Wave,
    Rainbow,
    Comet,
    Strobe,
    Spark,
    Audio,
    Custom,
}

impl Effect {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "static" => Some(Self::Static),
            "breathing" | "breathe" => Some(Self::Breathing),
            "wave" | "color-wave" | "colorwave" => Some(Self::Wave),
            "rainbow" | "cycle" => Some(Self::Rainbow),
            "comet" => Some(Self::Comet),
            "strobe" | "flash" => Some(Self::Strobe),
            "spark" | "starry" | "star" => Some(Self::Spark),
            "audio" | "visualizer" | "music" => Some(Self::Audio),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Breathing => "breathing",
            Self::Wave => "wave",
            Self::Rainbow => "rainbow",
            Self::Comet => "comet",
            Self::Strobe => "strobe",
            Self::Spark => "spark",
            Self::Audio => "audio",
            Self::Custom => "custom",
        }
    }
}

impl std::fmt::Display for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a custom color sequence moves across the four keyboard zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CustomMotion {
    #[default]
    Cycle,
    Chase,
    Pulse,
}

impl CustomMotion {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "cycle" => Some(Self::Cycle),
            "chase" => Some(Self::Chase),
            "pulse" => Some(Self::Pulse),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cycle => "cycle",
            Self::Chase => "chase",
            Self::Pulse => "pulse",
        }
    }
}

/// Colors a custom effect plays, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomSequence {
    pub motion: CustomMotion,
    pub steps: Vec<RgbColor>,
}

impl Default for CustomSequence {
    fn default() -> Self {
        Self {
            motion: CustomMotion::Cycle,
            steps: vec![
                RgbColor::new(255, 40, 20),
                RgbColor::new(255, 159, 26),
                RgbColor::new(48, 209, 88),
                RgbColor::new(10, 132, 255),
            ],
        }
    }
}

impl CustomSequence {
    /// Keeps 2–8 colors. A shorter list falls back to the built-in sequence.
    pub fn clamped(self) -> Self {
        if !(2..=8).contains(&self.steps.len()) {
            return Self::default();
        }
        Self {
            motion: self.motion,
            steps: self.steps,
        }
    }
}

/// How a software lighting effect moves. Static color ignores these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectStyle {
    /// 1 is slowest, 10 is fastest. 5 matches the original timing.
    pub speed: u8,
    /// 0 holds full brightness. 100 lets a pulse fade all the way out.
    pub depth: u8,
    /// 0 is white, 100 is a full color.
    pub saturation: u8,
    /// Flips the left-to-right sweep of Wave and Rainbow.
    pub reverse: bool,
}

impl Default for EffectStyle {
    fn default() -> Self {
        Self {
            speed: 5,
            depth: 80,
            saturation: 100,
            reverse: false,
        }
    }
}

impl EffectStyle {
    pub fn clamped(self) -> Self {
        Self {
            speed: self.speed.clamp(1, 10),
            depth: self.depth.min(100),
            saturation: self.saturation.min(100),
            reverse: self.reverse,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn parse_hex(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let value = u32::from_str_radix(hex, 16).ok()?;
        Some(Self {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        })
    }
}

/// One named fan curve stored by the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCurveInfo {
    pub name: String,
    pub hysteresis_secs: u32,
    /// `(°C, PWM 0–255)`.
    pub points: Vec<(u32, u32)>,
}

/// Snapshot the GUI, CLI, and Waybar all render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub daemon_ok: bool,
    pub cpu_c: u32,
    pub gpu_c: u32,
    pub fan1_rpm: u32,
    pub fan2_rpm: u32,
    pub pwm: u32,
    pub preset: String,
    pub profile: String,
    pub effect: String,
    pub rgb_available: bool,
    pub fan_control_available: bool,
    pub message: String,
    /// Active curve as `(°C, PWM 0–255)`. Empty when an older daemon omits it.
    #[serde(default)]
    pub curve: Vec<(u32, u32)>,
    /// Spin-down delay. Rising temperature still changes the fans immediately.
    #[serde(default = "default_hysteresis_secs")]
    pub hysteresis_secs: u32,
    /// Named curves. Empty when an older daemon omits them.
    #[serde(default)]
    pub saved: Vec<SavedCurveInfo>,
    /// Saved curve or preset used on wall power. Empty means manual.
    #[serde(default)]
    pub plugged_in: String,
    /// Saved curve or preset used on battery. Empty means manual.
    #[serde(default)]
    pub on_battery: String,
    /// `ac`, `battery`, or `unknown`.
    #[serde(default = "default_power")]
    pub power: String,
    /// This daemon understands saved curves. Older builds leave it false.
    #[serde(default)]
    pub library: bool,
    /// Lighting effect speed, 1–10. Older daemons leave the default.
    #[serde(default = "default_rgb_speed")]
    pub rgb_speed: u8,
    /// How far a pulse fades, 0–100.
    #[serde(default = "default_rgb_depth")]
    pub rgb_depth: u8,
    /// Color strength for Wave and Rainbow, 0–100.
    #[serde(default = "default_rgb_saturation")]
    pub rgb_saturation: u8,
    /// Wave and Rainbow travel from the right when this is set.
    #[serde(default)]
    pub rgb_reverse: bool,
    /// `cycle`, `chase`, or `pulse`. Older daemons omit this.
    #[serde(default = "default_custom_motion")]
    pub custom_motion: String,
    /// Custom-effect colors as `(r, g, b)`. Empty when an older daemon omits them.
    #[serde(default)]
    pub custom_steps: Vec<(u8, u8, u8)>,
    /// Keyboard lamp. False after the backlight key turns it off. Older daemons omit this.
    #[serde(default = "default_rgb_on")]
    pub rgb_on: bool,
}

fn default_rgb_on() -> bool {
    true
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

fn default_custom_motion() -> String {
    "cycle".into()
}

fn default_power() -> String {
    "unknown".into()
}

fn default_hysteresis_secs() -> u32 {
    4
}

impl Status {
    pub fn offline(message: impl Into<String>) -> Self {
        Self {
            daemon_ok: false,
            cpu_c: 0,
            gpu_c: 0,
            fan1_rpm: 0,
            fan2_rpm: 0,
            pwm: 0,
            preset: "unknown".into(),
            profile: "unknown".into(),
            effect: "unknown".into(),
            rgb_available: false,
            fan_control_available: false,
            message: message.into(),
            curve: Vec::new(),
            hysteresis_secs: default_hysteresis_secs(),
            saved: Vec::new(),
            plugged_in: String::new(),
            on_battery: String::new(),
            power: default_power(),
            library: false,
            rgb_speed: default_rgb_speed(),
            rgb_depth: default_rgb_depth(),
            rgb_saturation: default_rgb_saturation(),
            rgb_reverse: false,
            custom_motion: default_custom_motion(),
            custom_steps: Vec::new(),
            rgb_on: true,
        }
    }

    pub fn waybar_class(&self) -> &'static str {
        if !self.daemon_ok {
            return "error";
        }
        let hot = self.cpu_c.max(self.gpu_c);
        if hot >= 90 {
            "critical"
        } else if hot >= 80 {
            "warning"
        } else {
            "normal"
        }
    }
}
