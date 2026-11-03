// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Software lighting frames. The daemon and the on-screen preview share these
//! so a zone paints the same color the keyboard will show.

use crate::{CustomMotion, CustomSequence, Effect, EffectStyle, RgbColor, ZoneId};

/// Keep a lit frame above zero.
///
/// An all-zero write powers the keyboard lamp off, and the driver then ignores
/// color writes until the backlight key turns it on. Audio silence and the
/// bottom of a deep pulse used to take that path and stay dark.
fn scale_brightness(brightness: u8, level: f32) -> u8 {
    if brightness == 0 {
        return 0;
    }
    let scaled = (f32::from(brightness) * level.clamp(0.0, 1.0)).round() as u8;
    scaled.max(1)
}

pub fn effect_frame(
    effect: Effect,
    colors: &[(ZoneId, RgbColor)],
    tick: u64,
    brightness: u8,
    style: EffectStyle,
    custom: &CustomSequence,
) -> Vec<(ZoneId, RgbColor, u8)> {
    let style = style.clamped();
    let rate = f32::from(style.speed) / 5.0;
    let phase = tick as f32 * rate;
    let depth = f32::from(style.depth) / 100.0;
    let saturation = f32::from(style.saturation) / 100.0;
    let last = colors.len().saturating_sub(1);
    let fallback;
    let steps: &[RgbColor] = if custom.steps.len() >= 2 {
        &custom.steps
    } else {
        fallback = CustomSequence::default().steps;
        &fallback
    };
    let span = (16.0 / rate).max(4.0);
    let travel = phase / span;
    let step_index = travel.floor() as usize;
    let blend = depth * travel.fract();
    colors
        .iter()
        .enumerate()
        .map(|(index, (zone, color))| {
            let place = if style.reverse { last - index } else { index };
            let (color, brightness) = match effect {
                Effect::Static => (*color, brightness),
                Effect::Breathing => {
                    let wave = ((phase * 0.28).sin() + 1.0) / 2.0;
                    let level = 1.0 - depth * (1.0 - wave);
                    (*color, scale_brightness(brightness, level))
                }
                Effect::Rainbow => {
                    let hue = (phase * 12.0) as u64 + place as u64 * 48;
                    (hsv_to_rgb((hue % 360) as f32, saturation, 1.0), brightness)
                }
                Effect::Wave => {
                    let hue = (phase * 10.0) as u64 + place as u64 * 70;
                    let wave = ((phase * 0.45 + place as f32).sin() + 1.0) / 2.0;
                    let level = 1.0 - depth * (1.0 - wave);
                    (
                        hsv_to_rgb((hue % 360) as f32, saturation, 1.0),
                        scale_brightness(brightness, level),
                    )
                }
                Effect::Comet => {
                    let span = last.max(1) as f32;
                    let head = phase * 0.55 % (span + 1.0);
                    let glow = (1.0 - (place as f32 - head).abs() / 1.35).clamp(0.0, 1.0);
                    let level = 1.0 - depth * (1.0 - glow);
                    (*color, scale_brightness(brightness, level))
                }
                Effect::Strobe => {
                    let on = (phase * 1.6).sin() >= 0.0;
                    let level = if on { 1.0 } else { 1.0 - depth };
                    (*color, scale_brightness(brightness, level))
                }
                Effect::Spark => {
                    let seed = tick
                        .wrapping_mul(1_103_515_245)
                        .wrapping_add(place as u64 * 12_345);
                    let roll = (seed >> 8) % 100;
                    let on = roll < 22 + u64::from(style.speed) * 3;
                    let level = if on { 1.0 } else { (1.0 - depth) * 0.25 };
                    (*color, scale_brightness(brightness, level))
                }
                Effect::Audio => (*color, brightness),
                Effect::Custom => custom_step(
                    custom.motion,
                    steps,
                    CustomFrame {
                        index: step_index,
                        place,
                        blend,
                        depth,
                        phase,
                        saturation,
                        brightness,
                    },
                ),
            };
            (*zone, color, brightness)
        })
        .collect()
}

struct CustomFrame {
    index: usize,
    place: usize,
    blend: f32,
    depth: f32,
    phase: f32,
    saturation: f32,
    brightness: u8,
}

fn custom_step(motion: CustomMotion, steps: &[RgbColor], frame: CustomFrame) -> (RgbColor, u8) {
    let CustomFrame {
        index,
        place,
        blend,
        depth,
        phase,
        saturation,
        brightness,
    } = frame;
    let count = steps.len();
    let at = |offset: usize| steps[(index + offset) % count];
    let color = match motion {
        CustomMotion::Cycle => mix_rgb(at(0), at(1), blend),
        CustomMotion::Chase => mix_rgb(at(place), at(place + 1), blend),
        CustomMotion::Pulse => at(place),
    };
    let color = tint(color, saturation);
    let level = if motion == CustomMotion::Pulse {
        let wave = ((phase * 0.28 + place as f32 * 0.4).sin() + 1.0) / 2.0;
        1.0 - depth * (1.0 - wave)
    } else {
        1.0
    };
    (color, scale_brightness(brightness, level))
}

fn mix_rgb(from: RgbColor, to: RgbColor, t: f32) -> RgbColor {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    RgbColor::new(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
    )
}

fn tint(color: RgbColor, saturation: f32) -> RgbColor {
    if saturation >= 0.999 {
        return color;
    }
    let (hue, sat, value) = rgb_to_hsv(color);
    hsv_to_rgb(hue, sat * saturation, value)
}

/// Paint each zone from its saved color, brightening with a frequency band.
/// `levels` is bass, low, mid, high, in the same order as `colors` unless reversed.
pub fn audio_frame(
    colors: &[(ZoneId, RgbColor)],
    levels: &[u8],
    brightness: u8,
    style: EffectStyle,
) -> Vec<(ZoneId, RgbColor, u8)> {
    let style = style.clamped();
    let depth = f32::from(style.depth) / 100.0;
    let sat_scale = f32::from(style.saturation) / 100.0;
    let last = colors.len().saturating_sub(1);
    colors
        .iter()
        .enumerate()
        .map(|(index, (zone, color))| {
            let place = if style.reverse { last - index } else { index };
            let level = f32::from(*levels.get(place).unwrap_or(&0)) / 255.0;
            let (hue, saturation, _) = rgb_to_hsv(*color);
            let painted = hsv_to_rgb(hue, (saturation * sat_scale).clamp(0.0, 1.0), 1.0);
            let amount = 1.0 - depth * (1.0 - level.powf(1.4));
            (*zone, painted, scale_brightness(brightness, amount))
        })
        .collect()
}

pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> RgbColor {
    let c = value * saturation;
    let h = (hue / 60.0) % 6.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    RgbColor::new(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

pub fn rgb_to_hsv(color: RgbColor) -> (f32, f32, f32) {
    let r = f32::from(color.r) / 255.0;
    let g = f32::from(color.g) / 255.0;
    let b = f32::from(color.b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let mut hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    if hue < 0.0 {
        hue += 360.0;
    }
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    (hue, saturation, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_keeps_painted_hues_and_reverse_moves_frequencies_only() {
        let zones = [
            (ZoneId::Left, RgbColor::new(255, 0, 0)),
            (ZoneId::Wasd, RgbColor::new(0, 255, 0)),
            (ZoneId::Center, RgbColor::new(0, 0, 255)),
            (ZoneId::Right, RgbColor::new(255, 255, 0)),
        ];
        let style = EffectStyle {
            depth: 100,
            ..EffectStyle::default()
        };
        let full = audio_frame(&zones, &[255; 4], 200, style);
        for (frame, (_, selected)) in full.iter().zip(zones) {
            assert_eq!(frame.1, selected);
            assert_eq!(frame.2, 200);
        }
        let forward = audio_frame(&zones, &[255, 0, 0, 0], 200, style);
        let reverse = audio_frame(
            &zones,
            &[255, 0, 0, 0],
            200,
            EffectStyle {
                reverse: true,
                ..style
            },
        );
        assert_eq!(forward[0].2, 200);
        assert_eq!(reverse[3].2, 200);
        assert_eq!(reverse[0].2, 1);
        assert_eq!(reverse[3].1, zones[3].1);
    }

    #[test]
    fn breathing_dims_without_changing_hue_family() {
        let colors = [(ZoneId::Left, RgbColor::new(255, 0, 0))];
        let frame = effect_frame(
            Effect::Static,
            &colors,
            0,
            200,
            EffectStyle::default(),
            &CustomSequence::default(),
        );
        assert_eq!(frame[0].1, RgbColor::new(255, 0, 0));
        assert_eq!(frame[0].2, 200);
        let rainbow = effect_frame(
            Effect::Rainbow,
            &colors,
            0,
            200,
            EffectStyle::default(),
            &CustomSequence::default(),
        );
        assert_eq!(rainbow.len(), 1);
        let held = effect_frame(
            Effect::Breathing,
            &colors,
            17,
            200,
            EffectStyle {
                depth: 0,
                ..EffectStyle::default()
            },
            &CustomSequence::default(),
        );
        let faded = effect_frame(
            Effect::Breathing,
            &colors,
            17,
            200,
            EffectStyle::default(),
            &CustomSequence::default(),
        );
        assert_eq!(held[0].2, 200);
        assert!(faded[0].2 < held[0].2);
        let zones = [
            (ZoneId::Left, RgbColor::new(255, 0, 0)),
            (ZoneId::Right, RgbColor::new(0, 0, 255)),
        ];
        let forward = effect_frame(
            Effect::Rainbow,
            &zones,
            3,
            200,
            EffectStyle::default(),
            &CustomSequence::default(),
        );
        let reverse = effect_frame(
            Effect::Rainbow,
            &zones,
            3,
            200,
            EffectStyle {
                reverse: true,
                ..EffectStyle::default()
            },
            &CustomSequence::default(),
        );
        assert_ne!(forward[0].1, reverse[0].1);
        let loud_left = audio_frame(&zones, &[255, 0], 200, EffectStyle::default());
        assert!(loud_left[0].2 > loud_left[1].2);
        let sequence = CustomSequence {
            motion: CustomMotion::Cycle,
            steps: vec![RgbColor::new(255, 0, 0), RgbColor::new(0, 0, 255)],
        };
        let flat = EffectStyle {
            depth: 0,
            ..EffectStyle::default()
        };
        let first = effect_frame(Effect::Custom, &zones, 0, 200, flat, &sequence);
        let later = effect_frame(Effect::Custom, &zones, 16, 200, flat, &sequence);
        assert_eq!(first[0].1, RgbColor::new(255, 0, 0));
        assert_eq!(first[1].1, first[0].1);
        assert_eq!(later[0].1, RgbColor::new(0, 0, 255));
        let chase = CustomSequence {
            motion: CustomMotion::Chase,
            ..sequence
        };
        let walked = effect_frame(Effect::Custom, &zones, 0, 200, flat, &chase);
        assert_eq!(walked[0].1, RgbColor::new(255, 0, 0));
        assert_eq!(walked[1].1, RgbColor::new(0, 0, 255));
    }

    #[test]
    fn silence_keeps_the_lamp_ready_for_the_next_beat() {
        let zones = [
            (ZoneId::Left, RgbColor::new(255, 0, 0)),
            (ZoneId::Right, RgbColor::new(0, 0, 255)),
        ];
        let deep = EffectStyle {
            depth: 100,
            ..EffectStyle::default()
        };
        let silent = audio_frame(&zones, &[0, 0, 0, 0], 200, deep);
        assert!(silent.iter().all(|frame| frame.2 >= 1));
        let off = audio_frame(&zones, &[0, 0, 0, 0], 0, deep);
        assert!(off.iter().all(|frame| frame.2 == 0));
        let loud = audio_frame(&zones, &[255, 0], 200, deep);
        assert!(loud[0].2 > loud[1].2);
    }
}
