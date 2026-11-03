// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::Mutex;

use omen_common::{
    CustomMotion, CustomSequence, Effect, EffectStyle, RgbColor, Status, ZoneId, audio_frame,
    effect_frame,
};
use slint::VecModel;

use crate::{AppWindow, StepSwatch};

#[derive(Clone)]
pub(crate) struct Lights {
    pub(crate) zones: [RgbColor; 4],
    pub(crate) tick: u64,
    pub(crate) suppress: bool,
    pub(crate) style_ready: bool,
    pub(crate) synced_speed: f32,
    pub(crate) synced_depth: f32,
    pub(crate) synced_saturation: f32,
    pub(crate) synced_reverse: bool,
    pub(crate) audio: [u8; 4],
    pub(crate) custom_steps: Vec<RgbColor>,
    pub(crate) custom_motion: String,
    pub(crate) custom_ready: bool,
    pub(crate) synced_steps: Vec<RgbColor>,
    pub(crate) synced_motion: String,
    /// User choices waiting for the daemon to echo them. A status poll that
    /// still carries the previous value must not paint over these.
    pub(crate) pending_effect: Option<String>,
    pub(crate) pending_preset: Option<String>,
    pub(crate) pending_profile: Option<String>,
    pub(crate) pending_style: Option<StyleHold>,
    pub(crate) pending_custom: Option<(String, Vec<RgbColor>)>,
    pub(crate) last_paint_zone: String,
    pub(crate) lamp_on: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct StyleHold {
    pub(crate) speed: u8,
    pub(crate) depth: u8,
    pub(crate) saturation: u8,
    pub(crate) reverse: bool,
}

impl Lights {
    pub(crate) fn new() -> Self {
        Self {
            zones: [RgbColor::new(255, 40, 20); 4],
            tick: 0,
            suppress: false,
            style_ready: false,
            synced_speed: 5.0,
            synced_depth: 80.0,
            synced_saturation: 100.0,
            synced_reverse: false,
            audio: [0; 4],
            custom_steps: CustomSequence::default().steps,
            custom_motion: CustomMotion::Cycle.as_str().into(),
            custom_ready: false,
            synced_steps: Vec::new(),
            synced_motion: String::new(),
            pending_effect: None,
            pending_preset: None,
            pending_profile: None,
            pending_style: None,
            pending_custom: None,
            last_paint_zone: String::new(),
            lamp_on: true,
        }
    }
}

pub(crate) fn zone_slot(zone: &str) -> Option<usize> {
    match zone {
        "wasd" => Some(0),
        "left" => Some(1),
        "center" => Some(2),
        "right" => Some(3),
        _ => None,
    }
}

pub(crate) fn place_marker(ui: &AppWindow, color: RgbColor) {
    let (x, y) = crate::color::marker_for(color, 168.0);
    ui.set_marker_x(x);
    ui.set_marker_y(y);
}

pub(crate) fn shown(color: RgbColor, brightness: u8) -> slint::Color {
    let scale = u16::from(brightness);
    slint::Color::from_rgb_u8(
        (u16::from(color.r) * scale / 255) as u8,
        (u16::from(color.g) * scale / 255) as u8,
        (u16::from(color.b) * scale / 255) as u8,
    )
}

pub(crate) fn style_from_ui(ui: &AppWindow) -> EffectStyle {
    EffectStyle {
        speed: ui.get_speed().round().clamp(1.0, 10.0) as u8,
        depth: ui.get_depth().round().clamp(0.0, 100.0) as u8,
        saturation: ui.get_saturation().round().clamp(0.0, 100.0) as u8,
        reverse: ui.get_reverse(),
    }
}

pub(crate) fn paint_keys(ui: &AppWindow, lights: &Lights) {
    if !lights.lamp_on {
        let off = slint::Color::from_rgb_u8(0, 0, 0);
        ui.set_key_wasd(off);
        ui.set_key_left(off);
        ui.set_key_center(off);
        ui.set_key_right(off);
        return;
    }
    let effect = Effect::parse(ui.get_effect().as_str()).unwrap_or(Effect::Static);
    let brightness = channel(ui.get_brightness());
    let colors = [
        (ZoneId::Left, lights.zones[1]),
        (ZoneId::Wasd, lights.zones[0]),
        (ZoneId::Center, lights.zones[2]),
        (ZoneId::Right, lights.zones[3]),
    ];
    let style = style_from_ui(ui);
    let frames = if effect == Effect::Audio {
        audio_frame(&colors, &lights.audio, brightness, style)
    } else {
        effect_frame(
            effect,
            &colors,
            lights.tick,
            brightness,
            style,
            &CustomSequence {
                motion: CustomMotion::parse(&lights.custom_motion).unwrap_or(CustomMotion::Cycle),
                steps: lights.custom_steps.clone(),
            },
        )
    };
    for (zone, color, level) in frames {
        let paint = shown(color, level);
        match zone {
            ZoneId::Wasd => ui.set_key_wasd(paint),
            ZoneId::Left => ui.set_key_left(paint),
            ZoneId::Center => ui.set_key_center(paint),
            ZoneId::Right => ui.set_key_right(paint),
            ZoneId::All => {}
        }
    }
}

pub(crate) fn push_steps(ui: &AppWindow, steps: &[RgbColor], motion: &str) {
    let model = VecModel::from(
        steps
            .iter()
            .map(|color| StepSwatch {
                r: f32::from(color.r),
                g: f32::from(color.g),
                b: f32::from(color.b),
            })
            .collect::<Vec<_>>(),
    );
    ui.set_steps(slint::ModelRc::new(model));
    let picked = ui.get_step_picked();
    if picked < 0 || picked >= steps.len() as i32 {
        ui.set_step_picked(0);
    }
    ui.set_custom_motion(motion.into());
}

pub(crate) fn status_caught_up(pending: &mut Option<String>, incoming: &str) -> bool {
    match pending {
        Some(wanted) if wanted == incoming => {
            *pending = None;
            true
        }
        Some(_) => false,
        None => true,
    }
}

pub(crate) fn sync_held(ui: &AppWindow, status: &Status, lights: &Mutex<Lights>) {
    if !status.daemon_ok {
        return;
    }
    let (effect, preset, profile) = {
        let mut lights = lights.lock().expect("lights");
        (
            status_caught_up(&mut lights.pending_effect, &status.effect),
            status_caught_up(&mut lights.pending_preset, &status.preset),
            status_caught_up(&mut lights.pending_profile, &status.profile),
        )
    };
    if effect && ui.get_effect() != status.effect {
        ui.set_effect(status.effect.clone().into());
    }
    if preset && ui.get_preset() != status.preset {
        ui.set_preset(status.preset.clone().into());
    }
    if profile && ui.get_profile() != status.profile {
        ui.set_profile(status.profile.clone().into());
    }
}

pub(crate) fn sync_custom(ui: &AppWindow, status: &Status, lights: &Mutex<Lights>) {
    if !status.daemon_ok || status.custom_steps.len() < 2 {
        return;
    }
    let motion = if CustomMotion::parse(&status.custom_motion).is_some() {
        status.custom_motion.clone()
    } else {
        CustomMotion::Cycle.as_str().into()
    };
    let steps = status
        .custom_steps
        .iter()
        .map(|(r, g, b)| RgbColor::new(*r, *g, *b))
        .collect::<Vec<_>>();
    {
        let mut lights = lights.lock().expect("lights");
        if lights.pending_custom.is_some() {
            if lights.pending_custom.as_ref() == Some(&(motion.clone(), steps.clone())) {
                lights.pending_custom = None;
                lights.synced_steps = steps;
                lights.synced_motion = motion;
                lights.custom_ready = true;
            }
            return;
        }
    }
    let apply = {
        let mut lights = lights.lock().expect("lights");
        let untouched = !lights.custom_ready
            || (lights.custom_steps == lights.synced_steps
                && lights.custom_motion == lights.synced_motion);
        lights.synced_steps = steps.clone();
        lights.synced_motion = motion.clone();
        lights.custom_ready = true;
        if untouched {
            lights.custom_steps = steps.clone();
            lights.custom_motion = motion.clone();
        }
        untouched
    };
    if apply {
        push_steps(ui, &steps, &motion);
    }
}

pub(crate) fn sync_style(ui: &AppWindow, status: &Status, lights: &Mutex<Lights>) {
    if !status.daemon_ok {
        return;
    }
    let incoming = StyleHold {
        speed: status.rgb_speed,
        depth: status.rgb_depth,
        saturation: status.rgb_saturation,
        reverse: status.rgb_reverse,
    };
    {
        let mut lights = lights.lock().expect("lights");
        if lights.pending_style.is_some() {
            if lights.pending_style == Some(incoming) {
                lights.pending_style = None;
                lights.synced_speed = f32::from(incoming.speed);
                lights.synced_depth = f32::from(incoming.depth);
                lights.synced_saturation = f32::from(incoming.saturation);
                lights.synced_reverse = incoming.reverse;
                lights.style_ready = true;
            }
            return;
        }
    }
    let speed = f32::from(incoming.speed);
    let depth = f32::from(incoming.depth);
    let saturation = f32::from(incoming.saturation);
    let reverse = incoming.reverse;
    let untouched = {
        let mut lights = lights.lock().expect("lights");
        let untouched = !lights.style_ready
            || (ui.get_speed() == lights.synced_speed
                && ui.get_depth() == lights.synced_depth
                && ui.get_saturation() == lights.synced_saturation
                && ui.get_reverse() == lights.synced_reverse);
        lights.synced_speed = speed;
        lights.synced_depth = depth;
        lights.synced_saturation = saturation;
        lights.synced_reverse = reverse;
        lights.style_ready = true;
        if untouched {
            lights.suppress = true;
        }
        untouched
    };
    if !untouched {
        return;
    }
    ui.set_speed(speed);
    ui.set_depth(depth);
    ui.set_saturation(saturation);
    ui.set_reverse(reverse);
    lights.lock().expect("lights").suppress = false;
}

pub(crate) fn channel(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}
