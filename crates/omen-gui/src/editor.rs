// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::{Arc, Mutex};

use omen_common::{Config, Preset, Status, mains_online};
use slint::{ComponentHandle, Model, VecModel};

use crate::{AppWindow, CurveChoice, FanPoint};

pub(crate) const MAX_POINTS: usize = 8;
pub(crate) const HIT_PX: f32 = 16.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Point {
    pub(crate) temp: f32,
    pub(crate) pct: f32,
}

#[derive(Clone)]
pub(crate) struct Editor {
    pub(crate) points: Vec<Point>,
    pub(crate) applied: Vec<Point>,
    pub(crate) selected: usize,
    pub(crate) drag: Option<usize>,
    pub(crate) dirty: bool,
    pub(crate) axis_min: f32,
    pub(crate) axis_max: f32,
    pub(crate) hysteresis: f32,
    /// Curve sent to the daemon and not yet echoed back in status, as `(°C, PWM)`.
    pub(crate) pending_curve: Option<Vec<(u32, u32)>>,
}

pub(crate) fn nudge<H>(
    window: &AppWindow,
    editor: &Arc<Mutex<Editor>>,
    apply: fn(&mut Editor, f32),
    hook: H,
) where
    H: FnOnce(&AppWindow, Box<dyn FnMut(f32) + 'static>),
{
    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    hook(
        window,
        Box::new(move |delta| {
            let mut editor = ed.lock().expect("editor");
            apply(&mut editor, delta);
            if let Some(ui) = weak.upgrade() {
                push(&ui, &editor);
            }
        }),
    );
}

pub(crate) fn push(ui: &AppWindow, editor: &Editor) {
    let mut padded = editor.points.clone();
    if let Some(last) = padded.last().cloned() {
        while padded.len() < MAX_POINTS {
            padded.push(last.clone());
        }
    }
    let model = VecModel::from(
        padded
            .into_iter()
            .map(|point| FanPoint {
                temp: point.temp,
                pct: point.pct,
            })
            .collect::<Vec<_>>(),
    );
    ui.set_points(slint::ModelRc::new(model));
    ui.set_point_count(editor.points.len() as i32);
    ui.set_selected(editor.selected as i32);
    ui.set_dirty(editor.dirty);
    ui.set_axis_min(editor.axis_min);
    ui.set_axis_max(editor.axis_max);
    ui.set_axis_min_label(format!("{:.0} °C", editor.axis_min).into());
    ui.set_axis_max_label(format!("{:.0} °C", editor.axis_max).into());
    ui.set_response_label(format!("{:.0} s", editor.hysteresis).into());
    if let Some(point) = editor.points.get(editor.selected) {
        ui.set_selected_temp(format!("{:.1} °C", point.temp).into());
        ui.set_selected_pct(format!("{:.1} %", point.pct).into());
    }
}

pub(crate) fn push_preview(
    ui: &AppWindow,
    preset: &str,
    curve: &[(u32, u32)],
    curve_preset: &str,
    edited: &[Point],
) {
    let quiet = [
        (45.0, 17.6),
        (60.0, 31.4),
        (75.0, 51.0),
        (85.0, 74.5),
        (95.0, 100.0),
    ];
    let balanced = [
        (40.0, 27.5),
        (55.0, 43.1),
        (70.0, 66.7),
        (80.0, 86.3),
        (90.0, 100.0),
    ];
    let sampled: Vec<(f32, f32)> = if preset == "max" {
        vec![(20.0, 100.0), (120.0, 100.0)]
    } else if preset == "auto" {
        vec![]
    } else if preset == curve_preset && !curve.is_empty() {
        curve
            .iter()
            .map(|(temp, pwm)| (*temp as f32, *pwm as f32 / 255.0 * 100.0))
            .collect()
    } else if preset == "quiet" {
        quiet.to_vec()
    } else if preset == "custom" && !edited.is_empty() {
        edited.iter().map(|point| (point.temp, point.pct)).collect()
    } else if !edited.is_empty() {
        edited.iter().map(|point| (point.temp, point.pct)).collect()
    } else {
        balanced.to_vec()
    };
    let count = sampled.len();
    let mut padded = sampled;
    if let Some(last) = padded.last().copied() {
        while padded.len() < MAX_POINTS {
            padded.push(last);
        }
    }
    while padded.len() < MAX_POINTS {
        padded.push((40.0, 0.0));
    }
    let model = VecModel::from(
        padded
            .into_iter()
            .map(|(temp, pct)| FanPoint { temp, pct })
            .collect::<Vec<_>>(),
    );
    ui.set_preview(slint::ModelRc::new(model));
    ui.set_preview_count(count as i32);
    ui.set_curve_caption(
        match preset {
            "quiet" => "Quiet",
            "max" => "Max",
            "auto" => "BIOS",
            "custom" => "Custom",
            _ => "Balanced",
        }
        .into(),
    );
}

pub(crate) struct SavedEditor {
    pub(crate) editor: Editor,
    pub(crate) preset: String,
}

pub(crate) fn power_label() -> String {
    match mains_online() {
        Some(true) => "Wall power".into(),
        Some(false) => "Battery".into(),
        None => "Power source unknown".into(),
    }
}

pub(crate) fn library_choices(names: &[String]) -> Vec<CurveChoice> {
    let mut choices = vec![
        choice("", "Manual"),
        choice("quiet", "Quiet"),
        choice("balanced", "Balanced"),
        choice("max", "Max"),
        choice("auto", "BIOS"),
    ];
    for name in names {
        choices.push(choice(name, name));
    }
    choices
}

pub(crate) fn choice(id: &str, label: &str) -> CurveChoice {
    CurveChoice {
        id: id.into(),
        label: label.into(),
    }
}

pub(crate) fn push_library(ui: &AppWindow, names: &[String], plugged_in: &str, on_battery: &str) {
    ui.set_plugged_in(plugged_in.into());
    ui.set_on_battery(on_battery.into());
    ui.set_saved_count(names.len() as i32);
    let current = ui.get_saved_names();
    if current.row_count() == names.len()
        && current
            .iter()
            .zip(names)
            .all(|(current, name)| current.as_str() == name)
        && ui.get_choices().row_count() == names.len() + 5
    {
        return;
    }
    ui.set_saved_names(slint::ModelRc::new(slint::VecModel::from(
        names
            .iter()
            .map(|name| slint::SharedString::from(name.as_str()))
            .collect::<Vec<_>>(),
    )));
    ui.set_choices(slint::ModelRc::new(slint::VecModel::from(library_choices(
        names,
    ))));
}

pub(crate) fn lookup_saved(name: &str, latest: &Mutex<Status>) -> Option<(Vec<(u32, u32)>, u32)> {
    if let Ok(status) = latest.lock()
        && let Some(saved) = status
            .saved
            .iter()
            .find(|saved| saved.name.eq_ignore_ascii_case(name))
    {
        return Some((saved.points.clone(), saved.hysteresis_secs));
    }
    let config = load_fan_config()?;
    let saved = config.saved_curve(name)?;
    Some((
        saved
            .points
            .iter()
            .map(|point| (point.temp, point.pwm))
            .collect(),
        u32::try_from(saved.hysteresis_secs).unwrap_or(4),
    ))
}

pub(crate) fn points_from_dbus(points: &[(u32, u32)]) -> Vec<Point> {
    points
        .iter()
        .map(|(temp, pwm)| Point {
            temp: *temp as f32,
            pct: (*pwm as f32 / 255.0 * 100.0).clamp(0.0, 100.0),
        })
        .collect()
}

pub(crate) fn editor_from_config(config: &Config) -> Option<Editor> {
    let curve = match config.thermal.preset {
        Preset::Quiet => config.quiet_curve().ok()?,
        Preset::Custom => config.custom_curve().ok()?,
        Preset::Balanced | Preset::Max | Preset::Auto => config.balanced_curve().ok()?,
    };
    let points = points_from_dbus(&curve.as_dbus());
    if points.len() < 2 {
        return None;
    }
    Some(Editor {
        applied: points.clone(),
        points,
        selected: 0,
        drag: None,
        dirty: false,
        axis_min: 20.0,
        axis_max: 120.0,
        hysteresis: config.thermal.hysteresis_secs.clamp(1, 30) as f32,
        pending_curve: None,
    })
}

/// Curve last written by omend. The window itself keeps nothing, and a daemon
/// built before status grew a `curve` field never sends one back.
pub(crate) fn load_fan_config() -> Option<Config> {
    let path = [
        std::path::PathBuf::from("/etc/omend/config.toml"),
        std::path::PathBuf::from("config/omend.toml"),
    ]
    .into_iter()
    .find(|path| path.is_file())?;
    Config::load(&path).ok()
}

pub(crate) fn load_saved_editor() -> Option<SavedEditor> {
    let config = load_fan_config()?;
    let preset = config.thermal.preset.to_string();
    Some(SavedEditor {
        editor: editor_from_config(&config)?,
        preset,
    })
}

impl Editor {
    pub(crate) fn balanced() -> Self {
        let points = [
            (40.0, 70.0),
            (55.0, 110.0),
            (70.0, 170.0),
            (80.0, 220.0),
            (90.0, 255.0),
        ]
        .map(|(temp, pwm)| Point {
            temp,
            pct: pwm / 255.0 * 100.0,
        })
        .to_vec();
        Self {
            applied: points.clone(),
            points,
            selected: 2,
            drag: None,
            dirty: false,
            axis_min: 20.0,
            axis_max: 120.0,
            hysteresis: 4.0,
            pending_curve: None,
        }
    }

    pub(crate) fn import_if_changed(&mut self, status: &Status) -> bool {
        let points = points_from_dbus(&status.curve);
        if self.pending_curve.is_some() {
            if status.curve.len() >= 2 && self.pending_curve.as_ref() == Some(&status.curve) {
                self.pending_curve = None;
            }
            return false;
        }
        if points.len() < 2 || points == self.points {
            return false;
        }
        self.points = points;
        self.applied = self.points.clone();
        self.selected = self.selected.min(self.points.len() - 1);
        self.hysteresis = status.hysteresis_secs.clamp(1, 30) as f32;
        self.dirty = false;
        true
    }

    pub(crate) fn press(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if let Some(index) = self.hit(x, y, w, h) {
            self.selected = index;
            self.drag = Some(index);
            return;
        }
        if self.points.len() >= MAX_POINTS || w <= 1.0 || h <= 1.0 {
            self.drag = None;
            return;
        }
        let point = Point {
            temp: self.x_to_temp(x, w),
            pct: self.y_to_pct(y, h),
        };
        let index = self
            .points
            .iter()
            .position(|existing| existing.temp > point.temp)
            .unwrap_or(self.points.len());
        self.points.insert(index, point);
        self.separate_temps();
        self.selected = index;
        self.drag = Some(index);
        self.dirty = true;
    }

    pub(crate) fn drag_to(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let Some(index) = self.drag else {
            return;
        };
        let temp = self.clamp_temp(index, self.x_to_temp(x, w));
        let pct = self.y_to_pct(y, h);
        if let Some(point) = self.points.get_mut(index) {
            point.temp = temp;
            point.pct = pct;
        }
        self.selected = index;
        self.dirty = true;
    }

    pub(crate) fn nudge_temp(&mut self, delta: f32) {
        let index = self.selected;
        let next = self.clamp_temp(index, self.points[index].temp + delta);
        self.points[index].temp = next;
        self.dirty = true;
    }

    pub(crate) fn nudge_pct(&mut self, delta: f32) {
        let index = self.selected;
        self.points[index].pct = (self.points[index].pct + delta).clamp(0.0, 100.0);
        self.dirty = true;
    }

    pub(crate) fn nudge_response(&mut self, delta: f32) {
        self.hysteresis = (self.hysteresis + delta).clamp(1.0, 30.0);
        self.dirty = true;
    }

    pub(crate) fn nudge_axis_min(&mut self, delta: f32) {
        self.axis_min = (self.axis_min + delta).clamp(0.0, self.axis_max - 20.0);
    }

    pub(crate) fn nudge_axis_max(&mut self, delta: f32) {
        self.axis_max = (self.axis_max + delta).clamp(self.axis_min + 20.0, 140.0);
    }

    pub(crate) fn remove_selected(&mut self) {
        if self.points.len() <= 2 {
            return;
        }
        self.points.remove(self.selected);
        self.selected = self.selected.min(self.points.len() - 1);
        self.dirty = true;
    }

    pub(crate) fn cancel(&mut self) {
        self.points = self.applied.clone();
        self.selected = self.selected.min(self.points.len().saturating_sub(1));
        self.dirty = false;
        self.drag = None;
    }

    pub(crate) fn mark_applied(&mut self) {
        self.applied = self.points.clone();
        self.pending_curve = Some(self.dbus_points());
        self.dirty = false;
    }

    pub(crate) fn load_curve(&mut self, points: Vec<Point>, hysteresis: f32) {
        if points.len() < 2 {
            return;
        }
        self.points = points;
        self.selected = self.selected.min(self.points.len() - 1);
        self.hysteresis = hysteresis.clamp(1.0, 30.0);
        self.dirty = true;
        self.drag = None;
    }

    pub(crate) fn dbus_points(&self) -> Vec<(u32, u32)> {
        let mut points: Vec<(u32, u32)> = self
            .points
            .iter()
            .map(|point| {
                (
                    point.temp.round().clamp(0.0, 140.0) as u32,
                    (point.pct / 100.0 * 255.0).round().clamp(0.0, 255.0) as u32,
                )
            })
            .collect();
        points.sort_by_key(|point| point.0);
        for index in 1..points.len() {
            if points[index].0 <= points[index - 1].0 {
                points[index].0 = points[index - 1].0 + 1;
            }
        }
        points
    }

    pub(crate) fn encode(&self) -> String {
        self.points
            .iter()
            .map(|point| format!("{:.1}:{:.1}", point.temp, point.pct))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub(crate) fn decode(&mut self, text: &str) -> bool {
        let mut points = Vec::new();
        for part in text.split(|ch: char| ch == ',' || ch.is_whitespace()) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let Some((temp, pct)) = part.split_once(':') else {
                return false;
            };
            let Ok(temp) = temp.trim().parse::<f32>() else {
                return false;
            };
            let Ok(pct) = pct.trim().parse::<f32>() else {
                return false;
            };
            points.push(Point {
                temp,
                pct: pct.clamp(0.0, 100.0),
            });
        }
        if points.len() < 2 || points.len() > MAX_POINTS {
            return false;
        }
        points.sort_by(|a, b| a.temp.total_cmp(&b.temp));
        self.points = points;
        self.separate_temps();
        self.selected = 0;
        self.dirty = true;
        true
    }

    pub(crate) fn hit(&self, x: f32, y: f32, w: f32, h: f32) -> Option<usize> {
        self.points
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, point)| {
                let (px, py) = self.xy(point, w, h);
                ((px - x).hypot(py - y) <= HIT_PX).then_some(index)
            })
    }

    pub(crate) fn xy(&self, point: &Point, w: f32, h: f32) -> (f32, f32) {
        let span = (self.axis_max - self.axis_min).max(1.0);
        let x = (point.temp - self.axis_min) / span * w;
        let y = (1.0 - point.pct / 100.0) * h;
        (x, y)
    }

    pub(crate) fn x_to_temp(&self, x: f32, w: f32) -> f32 {
        let span = (self.axis_max - self.axis_min).max(1.0);
        (self.axis_min + (x / w.max(1.0)) * span).clamp(self.axis_min, self.axis_max)
    }

    pub(crate) fn y_to_pct(&self, y: f32, h: f32) -> f32 {
        ((1.0 - y / h.max(1.0)) * 100.0).clamp(0.0, 100.0)
    }

    pub(crate) fn clamp_temp(&self, index: usize, temp: f32) -> f32 {
        let lower = if index == 0 {
            self.axis_min
        } else {
            self.points[index - 1].temp + 1.0
        };
        let upper = if index + 1 == self.points.len() {
            self.axis_max
        } else {
            self.points[index + 1].temp - 1.0
        };
        temp.clamp(lower.min(upper), upper.max(lower))
    }

    pub(crate) fn separate_temps(&mut self) {
        for index in 1..self.points.len() {
            if self.points[index].temp <= self.points[index - 1].temp {
                self.points[index].temp = self.points[index - 1].temp + 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_library_retains_models_and_changed_names_refresh_them() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        struct TestPlatform;
        impl slint::platform::Platform for TestPlatform {
            fn create_window_adapter(
                &self,
            ) -> Result<std::rc::Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError>
            {
                Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
            }
        }
        slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
        let ui = AppWindow::new().unwrap();
        push_library(&ui, &["First".into()], "quiet", "balanced");
        let names = ui.get_saved_names();
        let choices = ui.get_choices();
        push_library(&ui, &["First".into()], "max", "quiet");
        assert!(names == ui.get_saved_names());
        assert!(choices == ui.get_choices());
        assert_eq!(ui.get_plugged_in().as_str(), "max");
        assert_eq!(ui.get_on_battery().as_str(), "quiet");
        // Same count, different names must still update both lists.
        push_library(&ui, &["Renamed".into()], "max", "quiet");
        assert!(names != ui.get_saved_names());
        assert_eq!(
            ui.get_saved_names().row_data(0).unwrap().as_str(),
            "Renamed"
        );
        assert_eq!(ui.get_choices().row_data(5).unwrap().id.as_str(), "Renamed");
    }

    #[test]
    fn saved_custom_curve_reopens_as_itself() {
        let mut config = Config::default();
        config.thermal.preset = Preset::Custom;
        config.thermal.hysteresis_secs = 9;
        let curve = omen_common::Curve::new(vec![(43, 35), (90, 255)]).unwrap();
        config.set_custom_curve(&curve);

        let editor = editor_from_config(&config).unwrap();
        assert_ne!(editor.points, Editor::balanced().points);
        assert_eq!(editor.points.len(), 2);
        assert_eq!(editor.points[0].temp, 43.0);
        assert!((editor.points[0].pct - 35.0 / 255.0 * 100.0).abs() < 0.01);
        assert_eq!(editor.hysteresis, 9.0);
        assert!(!editor.dirty);
        assert_eq!(editor.applied, editor.points);
    }
}
