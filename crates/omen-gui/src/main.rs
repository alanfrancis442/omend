// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

mod audio;
mod color;
mod editor;
mod host;
mod lights;
mod session;
mod settings;

use std::process::Command as Proc;
use std::rc::Rc;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use omen_common::{Effect, RgbColor, Status};
use slint::ComponentHandle;

pub(crate) use editor::*;
pub(crate) use host::*;
pub(crate) use lights::*;
pub(crate) use session::*;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    slint::set_xdg_app_id("omen-gui")?;
    let preferences = settings::Preferences::load(&window);
    let refresh_ms = [500, 1000, 2000, 5000][window.get_refresh_choice().clamp(0, 3) as usize];
    let refresh = Arc::new(AtomicU64::new(refresh_ms));
    window.set_wheel(color::color_wheel(168));
    place_marker(&window, RgbColor::new(255, 40, 20));
    let audio_feed = Arc::new(Mutex::new(audio::AudioFeed::default()));
    let saved = load_saved_editor();
    let preset = saved
        .as_ref()
        .map(|saved| saved.preset.clone())
        .unwrap_or_else(|| "balanced".into());
    let editor = Arc::new(Mutex::new(
        saved
            .map(|saved| saved.editor)
            .unwrap_or_else(Editor::balanced),
    ));
    push(&window, &editor.lock().expect("editor"));
    let (names, plugged_in, on_battery) = load_fan_config()
        .map(|config| {
            (
                config
                    .thermal
                    .saved
                    .iter()
                    .map(|saved| saved.name.clone())
                    .collect::<Vec<_>>(),
                config.thermal.plugged_in,
                config.thermal.on_battery,
            )
        })
        .unwrap_or_default();
    push_library(&window, &names, &plugged_in, &on_battery);

    let lights = Arc::new(Mutex::new(Lights::new()));
    let latest = Arc::new(Mutex::new(Status::offline("Looking for omend…")));
    let (tx, rx) = mpsc::channel::<Command>();
    let worker_status = Arc::clone(&latest);
    let worker_refresh = Arc::clone(&refresh);
    let worker_audio = Arc::clone(&audio_feed);
    std::thread::spawn(move || worker(worker_status, rx, worker_refresh, worker_audio));

    let ui_status = Arc::clone(&latest);
    let ui_editor = Arc::clone(&editor);
    let ui_lights = Arc::clone(&lights);
    let timer = Rc::new(slint::Timer::default());
    let weak = window.as_weak();
    let monitor = HostMonitor::start(Arc::clone(&refresh));
    apply_status(
        &window,
        &latest.lock().expect("status"),
        &HostSnapshot::default(),
    );
    window.set_preset(preset.clone().into());
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(refresh_ms),
        move || {
            let Some(status) = ui_status.lock().ok().map(|guard| guard.clone()) else {
                return;
            };
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if let Some(host) = monitor.snapshot() {
                apply_status(&ui, &status, &host);
                ui.set_power_label(host.power.into());
            }
            ui_lights.lock().expect("lights").lamp_on = status.rgb_on;
            sync_held(&ui, &status, &ui_lights);
            sync_style(&ui, &status, &ui_lights);
            sync_custom(&ui, &status, &ui_lights);
            if status.library {
                let names = status
                    .saved
                    .iter()
                    .map(|saved| saved.name.clone())
                    .collect::<Vec<_>>();
                push_library(&ui, &names, &status.plugged_in, &status.on_battery);
            }
            let edited = ui_editor.lock().expect("editor").points.clone();
            let hold_preset = ui_lights.lock().expect("lights").pending_preset.clone();
            if status.daemon_ok {
                let name = hold_preset.clone().unwrap_or_else(|| status.preset.clone());
                push_preview(&ui, &name, &status.curve, &status.preset, &edited);
            }
            let mut editor = ui_editor.lock().expect("editor");
            if status.daemon_ok
                && hold_preset.is_none()
                && !editor.dirty
                && editor.import_if_changed(&status)
            {
                push(&ui, &editor);
            }
        },
    );

    wire(&window, &editor, &tx, &latest, &lights);
    preferences.wire(&window, Rc::clone(&timer), refresh);
    {
        let lights = lights.lock().expect("lights");
        push_steps(&window, &lights.custom_steps, &lights.custom_motion);
        paint_keys(&window, &lights);
    }
    let light_timer = slint::Timer::default();
    let light_ui = window.as_weak();
    let light_state = Arc::clone(&lights);
    let light_audio = Arc::clone(&audio_feed);
    let mut animation_step = false;
    light_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(50),
        move || {
            let Some(ui) = light_ui.upgrade() else {
                return;
            };
            // The daemon animates the hardware independently. Hidden preview
            // models do not need rebuilding while the user scrolls other pages.
            if ui.get_page() != 2 {
                return;
            }
            let feed = light_audio.lock().expect("audio").snapshot();
            let mut lights = light_state.lock().expect("lights");
            lights.audio = feed.levels;
            let effect = Effect::parse(ui.get_effect().as_str()).unwrap_or(Effect::Static);
            if effect == Effect::Audio {
                ui.set_audio_label(feed.note.into());
                ui.set_audio_spectrum(slint::ModelRc::new(slint::VecModel::from(
                    feed.spectrum.map(f32::from).to_vec(),
                )));
                ui.set_audio_peaks(slint::ModelRc::new(slint::VecModel::from(
                    feed.peaks.map(f32::from).to_vec(),
                )));
                ui.set_audio_beat(feed.beat);
            }
            if !lights.lamp_on {
                paint_keys(&ui, &lights);
                return;
            }
            animation_step = !animation_step;
            if effect != Effect::Static && animation_step {
                lights.tick = lights.tick.wrapping_add(1);
            }
            paint_keys(&ui, &lights);
        },
    );
    push_preview(
        &window,
        &preset,
        &[],
        &preset,
        &editor.lock().expect("editor").points,
    );
    window
        .window()
        .set_size(slint::LogicalSize::new(1280.0, 800.0));
    window.run()?;
    preferences.flush(&window);
    Ok(())
}

fn wire(
    window: &AppWindow,
    editor: &Arc<Mutex<Editor>>,
    tx: &mpsc::Sender<Command>,
    latest: &Arc<Mutex<Status>>,
    lights: &Arc<Mutex<Lights>>,
) {
    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    window.on_graph_press(move |x, y, w, h| {
        let mut editor = ed.lock().expect("editor");
        editor.press(x, y, w, h);
        if let Some(ui) = weak.upgrade() {
            push(&ui, &editor);
        }
    });

    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    window.on_graph_drag(move |x, y, w, h| {
        let mut editor = ed.lock().expect("editor");
        editor.drag_to(x, y, w, h);
        if let Some(ui) = weak.upgrade() {
            push(&ui, &editor);
        }
    });

    let ed = Arc::clone(editor);
    window.on_graph_release(move || {
        ed.lock().expect("editor").drag = None;
    });

    nudge(
        window,
        editor,
        |editor, delta| editor.nudge_temp(delta),
        |ui, cb| ui.on_nudge_temp(cb),
    );
    nudge(
        window,
        editor,
        |editor, delta| editor.nudge_pct(delta),
        |ui, cb| ui.on_nudge_pct(cb),
    );
    nudge(
        window,
        editor,
        |editor, delta| editor.nudge_response(delta),
        |ui, cb| {
            ui.on_nudge_response(cb);
        },
    );
    nudge(
        window,
        editor,
        |editor, delta| editor.nudge_axis_min(delta),
        |ui, cb| {
            ui.on_nudge_axis_min(cb);
        },
    );
    nudge(
        window,
        editor,
        |editor, delta| editor.nudge_axis_max(delta),
        |ui, cb| {
            ui.on_nudge_axis_max(cb);
        },
    );

    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    window.on_request_remove_point(move || {
        let mut editor = ed.lock().expect("editor");
        editor.remove_selected();
        if let Some(ui) = weak.upgrade() {
            push(&ui, &editor);
        }
    });

    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    let apply_tx = tx.clone();
    window.on_request_apply(move || {
        let editor = ed.lock().expect("editor");
        let _ = apply_tx.send(Command::Curve {
            points: editor.dbus_points(),
            hysteresis: editor.hysteresis.round().clamp(1.0, 30.0) as u32,
        });
        drop(editor);
        if let Some(ui) = weak.upgrade() {
            let mut editor = ed.lock().expect("editor");
            editor.mark_applied();
            push(&ui, &editor);
        }
    });

    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    window.on_request_cancel(move || {
        let mut editor = ed.lock().expect("editor");
        editor.cancel();
        if let Some(ui) = weak.upgrade() {
            push(&ui, &editor);
        }
    });

    let ed = Arc::clone(editor);
    window.on_request_copy(move || {
        let editor = ed.lock().expect("editor");
        let _ = Proc::new("wl-copy").arg(editor.encode()).status();
    });

    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    window.on_request_paste(move || {
        let Ok(output) = Proc::new("wl-paste").output() else {
            return;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let mut editor = ed.lock().expect("editor");
        if editor.decode(text.trim())
            && let Some(ui) = weak.upgrade()
        {
            push(&ui, &editor);
        }
    });

    let preset_tx = tx.clone();
    let weak = window.as_weak();
    let preview_status = Arc::clone(latest);
    let preview_editor = Arc::clone(editor);
    let preset_lights = Arc::clone(lights);
    window.on_request_preset(move |preset| {
        let name = preset.to_string();
        preset_lights.lock().expect("lights").pending_preset = Some(name.clone());
        if let Some(ui) = weak.upgrade() {
            let (curve, curve_preset) = preview_status
                .lock()
                .map(|status| (status.curve.clone(), status.preset.clone()))
                .unwrap_or_default();
            let edited = preview_editor
                .lock()
                .map(|editor| editor.points.clone())
                .unwrap_or_default();
            ui.set_preset(name.clone().into());
            push_preview(&ui, &name, &curve, &curve_preset, &edited);
        }
        let _ = preset_tx.send(Command::Preset(name));
    });
    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    let save_tx = tx.clone();
    window.on_request_save_curve(move |name| {
        let mut editor = ed.lock().expect("editor");
        let _ = save_tx.send(Command::SaveCurve {
            name: name.to_string(),
            points: editor.dbus_points(),
            hysteresis: editor.hysteresis.round().clamp(1.0, 30.0) as u32,
        });
        editor.mark_applied();
        if let Some(ui) = weak.upgrade() {
            push(&ui, &editor);
        }
    });

    let delete_tx = tx.clone();
    window.on_request_delete_curve(move |name| {
        let _ = delete_tx.send(Command::DeleteCurve(name.to_string()));
    });

    let weak = window.as_weak();
    let ed = Arc::clone(editor);
    let load_status = Arc::clone(latest);
    window.on_request_load_curve(move |name| {
        let Some((points, hysteresis)) = lookup_saved(name.as_str(), &load_status) else {
            return;
        };
        let mut editor = ed.lock().expect("editor");
        editor.load_curve(points_from_dbus(&points), hysteresis as f32);
        if let Some(ui) = weak.upgrade() {
            ui.set_curve_name(name);
            push(&ui, &editor);
        }
    });

    let power_tx = tx.clone();
    window.on_request_power_curve(move |source, name| {
        let _ = power_tx.send(Command::PowerCurve {
            source: source.to_string(),
            name: name.to_string(),
        });
    });

    let profile_tx = tx.clone();
    let profile_lights = Arc::clone(lights);
    let weak = window.as_weak();
    window.on_request_profile(move |profile| {
        let name = profile.to_string();
        profile_lights.lock().expect("lights").pending_profile = Some(name.clone());
        if let Some(ui) = weak.upgrade() {
            ui.set_profile(name.clone().into());
        }
        let _ = profile_tx.send(Command::Profile(name));
    });
    let effect_tx = tx.clone();
    let effect_lights = Arc::clone(lights);
    window.on_request_effect(move |effect| {
        let name = effect.to_string();
        effect_lights.lock().expect("lights").pending_effect = Some(name.clone());
        let _ = effect_tx.send(Command::Effect(name));
    });
    let style_tx = tx.clone();
    let style_lights = Arc::clone(lights);
    window.on_request_style(move |speed, depth, saturation, reverse| {
        let hold = StyleHold {
            speed: speed.round().clamp(1.0, 10.0) as u8,
            depth: depth.round().clamp(0.0, 100.0) as u8,
            saturation: saturation.round().clamp(0.0, 100.0) as u8,
            reverse,
        };
        style_lights.lock().expect("lights").pending_style = Some(hold);
        let _ = style_tx.send(Command::Style {
            speed: hold.speed,
            depth: hold.depth,
            saturation: hold.saturation,
            reverse: hold.reverse,
        });
    });
    let bright_tx = tx.clone();
    window.on_request_brightness(move |brightness| {
        let _ = bright_tx.send(Command::Brightness(channel(brightness)));
    });
    let weak = window.as_weak();
    let paint_lights = Arc::clone(lights);
    let paint_tx = tx.clone();
    window.on_paint_zone(move |zone| {
        let zone = zone.to_string();
        if zone.is_empty() {
            return;
        }
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let color = RgbColor::new(
            channel(ui.get_red()),
            channel(ui.get_green()),
            channel(ui.get_blue()),
        );
        let brightness = channel(ui.get_brightness());
        let mut lights = paint_lights.lock().expect("lights");
        if lights.last_paint_zone == zone {
            return;
        }
        lights.last_paint_zone = zone.clone();
        if let Some(slot) = zone_slot(&zone) {
            lights.zones[slot] = color;
        }
        drop(lights);
        ui.set_zone(zone.clone().into());
        paint_keys(&ui, &paint_lights.lock().expect("lights"));
        queue_rgb(&ui, &paint_lights, &paint_tx, zone, color, brightness);
    });
    let paint_lights = Arc::clone(lights);
    window.on_paint_release(move || {
        paint_lights.lock().expect("lights").last_paint_zone.clear();
    });
    let rgb_tx = tx.clone();
    let rgb_lights = Arc::clone(lights);
    let weak = window.as_weak();
    window.on_request_rgb(move |zone, r, g, b, brightness| {
        if let Some(ui) = weak.upgrade() {
            queue_rgb(
                &ui,
                &rgb_lights,
                &rgb_tx,
                zone.to_string(),
                RgbColor::new(channel(r), channel(g), channel(b)),
                channel(brightness),
            );
        }
    });

    let weak = window.as_weak();
    let preview = Arc::clone(lights);
    window.on_zone_changed(move |zone| {
        let Some(slot) = zone_slot(zone.as_str()) else {
            return;
        };
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let color = {
            let mut lights = preview.lock().expect("lights");
            lights.suppress = true;
            lights.zones[slot]
        };
        ui.set_red(f32::from(color.r));
        ui.set_green(f32::from(color.g));
        ui.set_blue(f32::from(color.b));
        let lights = {
            let mut lights = preview.lock().expect("lights");
            lights.suppress = false;
            lights.clone()
        };
        paint_keys(&ui, &lights);
        place_marker(&ui, color);
    });

    let weak = window.as_weak();
    window.on_wheel_at(move |x, y, w, h| {
        let (color, marker_x, marker_y) = color::color_at(x, y, w, h);
        let Some(ui) = weak.upgrade() else {
            return;
        };
        ui.set_marker_x(marker_x);
        ui.set_marker_y(marker_y);
        ui.set_red(f32::from(color.r));
        ui.set_green(f32::from(color.g));
        ui.set_blue(f32::from(color.b));
    });

    let weak = window.as_weak();
    let wheel_tx = tx.clone();
    let wheel_lights = Arc::clone(lights);
    window.on_wheel_release(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        queue_rgb(
            &ui,
            &wheel_lights,
            &wheel_tx,
            ui.get_zone().to_string(),
            RgbColor::new(
                channel(ui.get_red()),
                channel(ui.get_green()),
                channel(ui.get_blue()),
            ),
            channel(ui.get_brightness()),
        );
    });

    let weak = window.as_weak();
    let pick_tx = tx.clone();
    let pick_lights = Arc::clone(lights);
    window.on_pick_color(move |r, g, b| {
        let color = RgbColor::new(channel(r), channel(g), channel(b));
        let Some(ui) = weak.upgrade() else {
            return;
        };
        place_marker(&ui, color);
        queue_rgb(
            &ui,
            &pick_lights,
            &pick_tx,
            ui.get_zone().to_string(),
            color,
            channel(ui.get_brightness()),
        );
    });

    let weak = window.as_weak();
    let preview = Arc::clone(lights);
    let custom_tx = tx.clone();
    window.on_custom_edit(move |action| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let color = RgbColor::new(
            channel(ui.get_red()),
            channel(ui.get_green()),
            channel(ui.get_blue()),
        );
        let motion = ui.get_custom_motion().to_string();
        let mut lights = preview.lock().expect("lights");
        match action.as_str() {
            "add" if lights.custom_steps.len() < 8 => lights.custom_steps.push(color),
            "remove" if lights.custom_steps.len() > 2 => {
                let index = ui
                    .get_step_picked()
                    .clamp(0, lights.custom_steps.len() as i32 - 1)
                    as usize;
                lights.custom_steps.remove(index);
            }
            "paint" if !lights.custom_steps.is_empty() => {
                let index = ui
                    .get_step_picked()
                    .clamp(0, lights.custom_steps.len() as i32 - 1)
                    as usize;
                lights.custom_steps[index] = color;
            }
            "motion" => {}
            _ => return,
        }
        lights.custom_motion = motion;
        let steps = lights.custom_steps.clone();
        let motion = lights.custom_motion.clone();
        lights.pending_custom = Some((motion.clone(), steps.clone()));
        drop(lights);
        push_steps(&ui, &steps, &motion);
        paint_keys(&ui, &preview.lock().expect("lights"));
        let _ = custom_tx.send(Command::Custom { motion, steps });
    });

    let weak = window.as_weak();
    let preview = Arc::clone(lights);
    window.on_color_changed(move |zone, r, g, b, _brightness| {
        let mut lights = preview.lock().expect("lights");
        if lights.suppress {
            return;
        }
        let color = RgbColor::new(channel(r), channel(g), channel(b));
        if let Some(slot) = zone_slot(zone.as_str()) {
            lights.zones[slot] = color;
        } else {
            lights.zones = [color; 4];
        }
        let Some(ui) = weak.upgrade() else {
            return;
        };
        paint_keys(&ui, &lights);
    });
}

fn queue_rgb(
    ui: &AppWindow,
    lights: &Mutex<Lights>,
    tx: &mpsc::Sender<Command>,
    zone: String,
    color: RgbColor,
    brightness: u8,
) {
    let effect = ui.get_effect().to_string();
    lights.lock().expect("lights").pending_effect = Some(effect.clone());
    let _ = tx.send(Command::Rgb {
        zone,
        r: color.r,
        g: color.g,
        b: color.b,
        brightness,
        effect,
    });
}

fn apply_status(ui: &AppWindow, status: &Status, host: &HostSnapshot) {
    ui.set_cpu(format!("{}°C", status.cpu_c).into());
    ui.set_gpu(format!("{}°C", status.gpu_c).into());
    ui.set_fan1(format!("{}", status.fan1_rpm).into());
    ui.set_fan2(format!("{}", status.fan2_rpm).into());
    ui.set_pwm(status.pwm.to_string().into());
    ui.set_pwm_value(status.pwm as f32);
    ui.set_cpu_temp(status.cpu_c as f32);
    ui.set_gpu_temp(status.gpu_c as f32);
    ui.set_fan1_value(status.fan1_rpm as f32);
    ui.set_fan2_value(status.fan2_rpm as f32);
    ui.set_note(status.message.clone().into());
    ui.set_live_temp(status.cpu_c.max(status.gpu_c) as f32);
    ui.set_cpu_load(host.cpu);
    ui.set_gpu_load(host.gpu);
    ui.set_mem_load(host.mem);
    ui.set_cpu_load_label(percent_label(host.cpu));
    ui.set_gpu_load_label(percent_label(host.gpu));
    ui.set_mem_load_label(percent_label(host.mem));
    ui.set_mem_detail(host.mem_detail.clone().into());
    ui.set_disk_used(host.disk);
    ui.set_disk_label(host.disk_detail.clone().into());
}

#[cfg(test)]
mod lighting_tests {
    use super::*;

    #[test]
    fn palette_wheel_apply_and_keyboard_paint_preserve_audio_mode() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        struct TestPlatform(Rc<MinimalSoftwareWindow>);
        impl slint::platform::Platform for TestPlatform {
            fn create_window_adapter(
                &self,
            ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
                Ok(self.0.clone())
            }
        }
        let adapter = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(TestPlatform(adapter.clone()))).unwrap();
        let ui = AppWindow::new().unwrap();
        let editor = Arc::new(Mutex::new(Editor::balanced()));
        let latest = Arc::new(Mutex::new(Status::offline("test")));
        let lights = Arc::new(Mutex::new(Lights::new()));
        let (tx, rx) = mpsc::channel();
        wire(&ui, &editor, &tx, &latest, &lights);
        ui.set_effect("audio".into());
        ui.invoke_pick_color(0.0, 255.0, 0.0);
        ui.invoke_wheel_release();
        ui.invoke_request_rgb("center".into(), 0.0, 0.0, 255.0, 200.0);
        ui.invoke_paint_zone("left".into());
        for _ in 0..4 {
            match rx.try_recv().unwrap() {
                Command::Rgb { effect, .. } => assert_eq!(effect, "audio"),
                _ => panic!("a color edit must not queue a mode change"),
            }
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(ui.get_effect().as_str(), "audio");
        let mut old_status = Status::offline("old daemon response");
        old_status.daemon_ok = true;
        old_status.effect = "static".into();
        sync_held(&ui, &old_status, &lights);
        assert_eq!(ui.get_effect().as_str(), "audio");
        old_status.effect = "audio".into();
        sync_held(&ui, &old_status, &lights);
        assert!(lights.lock().unwrap().pending_effect.is_none());

        // Exercise real layout and software rasterization without a compositor,
        // D-Bus connection, config load, system sensors or background workers.
        // Keep this in the existing platform test to avoid extra global setup.
        let mut demo = Status::offline("SYNTHETIC DEMO — no live hardware data");
        demo.daemon_ok = true;
        demo.cpu_c = 58;
        demo.gpu_c = 48;
        demo.fan1_rpm = 3000;
        demo.fan2_rpm = 3200;
        demo.pwm = 130;
        demo.preset = "balanced".into();
        demo.profile = "balanced".into();
        demo.effect = "static".into();
        let demo_host = HostSnapshot {
            cpu: 17.0,
            gpu: 11.0,
            mem: 50.0,
            mem_detail: "8.0 / 16.0 GiB · synthetic".into(),
            disk: 32.0,
            disk_detail: "83 / 256 GiB · synthetic".into(),
            power: "AC · synthetic".into(),
        };
        apply_status(&ui, &demo, &demo_host);
        push(&ui, &Editor::balanced());
        ui.set_preset("balanced".into());
        ui.set_profile("balanced".into());
        ui.set_power_label("AC · synthetic".into());
        ui.set_effect("static".into());
        ui.set_preferences_path("Synthetic demo preferences".into());
        ui.set_preferences_message("SYNTHETIC DEMO — no live hardware data".into());
        ui.show().unwrap();
        for (layout, width, height) in [("compact", 480, 800), ("desktop", 1280, 800)] {
            adapter.set_size(slint::PhysicalSize::new(width, height));
            let mut page_fingerprints = std::collections::HashSet::new();
            for (page, name) in [(0, "dashboard"), (4, "settings"), (5, "about")] {
                ui.set_page(page);
                slint::platform::update_timers_and_animations();
                ui.window().request_redraw();
                let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
                let mut region = None;
                assert!(adapter.draw_if_needed(|renderer| {
                    region = Some(renderer.render(pixels.make_mut_slice(), width as usize));
                }));
                assert_eq!(
                    region.unwrap().bounding_box_size(),
                    slint::PhysicalSize::new(width, height)
                );
                let colors = pixels
                    .as_slice()
                    .iter()
                    .map(|pixel| (pixel.r, pixel.g, pixel.b))
                    .collect::<std::collections::HashSet<_>>();
                assert!(
                    colors.len() > 32,
                    "{layout}/{name} should contain rendered text and controls"
                );
                use std::hash::{Hash, Hasher};
                let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
                pixels.as_bytes().hash(&mut fingerprint);
                assert!(
                    page_fingerprints.insert(fingerprint.finish()),
                    "{layout}/{name} rendered the same frame as another page"
                );
                if let Some(directory) = std::env::var_os("OMEND_GUI_PREVIEW_DIR") {
                    use std::io::Write;
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    let mut image = std::fs::File::create(
                        directory.join(format!("synthetic-{name}-{layout}.ppm")),
                    )
                    .unwrap();
                    write!(
                        image,
                        "P6\n# SYNTHETIC DEMO - NO LIVE HARDWARE DATA\n{width} {height}\n255\n"
                    )
                    .unwrap();
                    image.write_all(pixels.as_bytes()).unwrap();
                }
            }
        }
        ui.hide().unwrap();
    }
}
