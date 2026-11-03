// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use omen_common::{OmendProxy, RgbColor, Status, ZoneId, connect};

pub(crate) enum Command {
    Preset(String),
    Profile(String),
    Effect(String),
    Style {
        speed: u8,
        depth: u8,
        saturation: u8,
        reverse: bool,
    },
    Custom {
        motion: String,
        steps: Vec<RgbColor>,
    },
    Rgb {
        zone: String,
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
        effect: String,
    },
    Brightness(u8),
    Curve {
        points: Vec<(u32, u32)>,
        hysteresis: u32,
    },
    SaveCurve {
        name: String,
        points: Vec<(u32, u32)>,
        hysteresis: u32,
    },
    DeleteCurve(String),
    PowerCurve {
        source: String,
        name: String,
    },
}

pub(crate) fn worker(
    latest: Arc<Mutex<Status>>,
    rx: mpsc::Receiver<Command>,
    refresh: Arc<AtomicU64>,
    audio_feed: Arc<Mutex<crate::audio::AudioFeed>>,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async move {
        let _audio_client = tokio::spawn(crate::audio::watch(audio_feed));
        loop {
            match connect(false).await {
                Ok(connection) => match OmendProxy::new(&connection).await {
                    Ok(proxy) => session(&proxy, &latest, &rx, &refresh).await,
                    Err(err) => store(&latest, Status::offline(err.to_string())),
                },
                Err(err) => store(&latest, Status::offline(err.to_string())),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

async fn session(
    proxy: &OmendProxy<'_>,
    latest: &Arc<Mutex<Status>>,
    rx: &mpsc::Receiver<Command>,
    refresh: &AtomicU64,
) {
    let mut notice: Option<(String, std::time::Instant)> = None;
    let mut last_poll: Option<std::time::Instant> = None;
    loop {
        let mut command_applied = false;
        while let Ok(command) = rx.try_recv() {
            if let Err(err) = apply_command(proxy, command).await {
                notice = Some((err, std::time::Instant::now()));
            } else {
                notice = None;
            }
            command_applied = true;
        }
        let interval = Duration::from_millis(refresh.load(Ordering::Relaxed));
        if !command_applied && last_poll.is_some_and(|last| last.elapsed() < interval) {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        }
        last_poll = Some(std::time::Instant::now());
        match proxy.get_status().await {
            Ok(raw) => match serde_json::from_str::<Status>(&raw) {
                Ok(mut status) => {
                    if let Some((err, at)) = notice.clone() {
                        if at.elapsed() < Duration::from_secs(6) {
                            status.message = err;
                        } else {
                            notice = None;
                        }
                    }
                    store(latest, status);
                }
                Err(err) => store(latest, Status::offline(err.to_string())),
            },
            Err(err) => {
                store(latest, Status::offline(err.to_string()));
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn apply_command(proxy: &OmendProxy<'_>, command: Command) -> Result<(), String> {
    match command {
        Command::Preset(name) => proxy
            .set_preset(&name)
            .await
            .map_err(|err| err.to_string())?,
        Command::Profile(name) => proxy
            .set_platform_profile(&name)
            .await
            .map_err(|err| err.to_string())?,
        Command::Effect(name) => proxy
            .set_rgb_effect(&name)
            .await
            .map_err(|err| err.to_string())?,
        Command::Style {
            speed,
            depth,
            saturation,
            reverse,
        } => {
            if let Err(err) = proxy.set_rgb_style(speed, depth, saturation, reverse).await {
                let message = err.to_string();
                if !message.contains("UnknownMethod") && !message.contains("unknown method") {
                    return Err(message);
                }
            }
        }
        Command::Custom { motion, steps } => {
            let packed = steps
                .iter()
                .map(|color| (color.r, color.g, color.b))
                .collect();
            if let Err(err) = proxy.set_custom_effect(&motion, packed).await {
                let message = err.to_string();
                if !message.contains("UnknownMethod") && !message.contains("unknown method") {
                    return Err(message);
                }
            }
        }
        Command::Rgb {
            zone,
            r,
            g,
            b,
            brightness,
            effect,
        } => {
            let zone_id = ZoneId::parse(&zone).ok_or_else(|| format!("unknown zone {zone}"))? as u8;
            proxy
                .set_rgb_zone(zone_id, r, g, b, brightness)
                .await
                .map_err(|err| err.to_string())?;
            // Older daemons made every color write switch to Static. Restore
            // the chosen effect as part of the same queued user operation.
            proxy
                .set_rgb_effect(&effect)
                .await
                .map_err(|err| err.to_string())?;
        }
        Command::Brightness(brightness) => {
            if let Err(err) = proxy.set_rgb_brightness(brightness).await {
                let message = err.to_string();
                if !message.contains("UnknownMethod") && !message.contains("unknown method") {
                    return Err(message);
                }
            }
        }
        Command::Curve { points, hysteresis } => {
            proxy
                .set_fan_curve(points)
                .await
                .map_err(|err| err.to_string())?;
            if let Err(err) = proxy.set_hysteresis(hysteresis).await {
                let message = err.to_string();
                if !message.contains("UnknownMethod") && !message.contains("unknown method") {
                    return Err(message);
                }
            }
        }
        Command::SaveCurve {
            name,
            points,
            hysteresis,
        } => {
            proxy
                .save_fan_curve(&name, points, hysteresis)
                .await
                .map_err(|err| err.to_string())?;
        }
        Command::DeleteCurve(name) => {
            proxy
                .delete_fan_curve(&name)
                .await
                .map_err(|err| err.to_string())?;
        }
        Command::PowerCurve { source, name } => {
            proxy
                .set_power_curve(&source, &name)
                .await
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn store(latest: &Mutex<Status>, status: Status) {
    if let Ok(mut guard) = latest.lock() {
        *guard = status;
    }
}
