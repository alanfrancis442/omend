// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use omen_audio::{AudioCapture, AudioFeed, SERVICE_NAME, SERVICE_PATH};
use omen_common::{OmendProxy, Status, connect};
use tokio::time::{Instant, MissedTickBehavior, timeout};

const RPC_TIMEOUT: Duration = Duration::from_millis(800);

struct AudioService {
    feed: Arc<Mutex<AudioFeed>>,
}

#[zbus::interface(name = "dev.omend.Audio")]
impl AudioService {
    fn get_snapshot(&self) -> zbus::fdo::Result<String> {
        let feed = self
            .feed
            .lock()
            .map_err(|err| zbus::fdo::Error::Failed(err.to_string()))?;
        serde_json::to_string(&feed.snapshot())
            .map_err(|err| zbus::fdo::Error::Failed(err.to_string()))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "omen-audio: user-session speaker spectrum service for omend\nRun with systemctl --user enable --now omen-audio.service"
        );
        return Ok(());
    }
    let feed = Arc::new(Mutex::new(AudioFeed::default()));
    // A session-bus name gives us one producer per user, independently of how
    // many GUI windows are open. No GUI connection owns this process.
    let _connection = zbus::connection::Builder::session()?
        .name(SERVICE_NAME)?
        .serve_at(
            SERVICE_PATH,
            AudioService {
                feed: Arc::clone(&feed),
            },
        )?
        .build()
        .await
        .context("own the background audio session-bus name")?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = run(feed) => {},
        result = tokio::signal::ctrl_c() => { result?; },
        _ = terminate.recv() => {},
    }
    Ok(())
}

async fn run(feed: Arc<Mutex<AudioFeed>>) {
    loop {
        let result = async {
            let connection = timeout(RPC_TIMEOUT, connect(false)).await??;
            let proxy = OmendProxy::new(&connection).await?;
            follow(&proxy, &feed).await
        }
        .await;
        if let Err(err) = result {
            feed.lock()
                .expect("audio feed")
                .clear(format!("Waiting for omend: {err}"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn default_sink() -> Option<String> {
    let output = timeout(
        Duration::from_millis(500),
        tokio::process::Command::new("pactl")
            .arg("get-default-sink")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let sink = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!sink.is_empty()).then_some(sink)
}

async fn follow(proxy: &OmendProxy<'_>, feed: &Arc<Mutex<AudioFeed>>) -> anyhow::Result<()> {
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut capture: Option<AudioCapture> = None;
    let mut active = false;
    let mut next_status = Instant::now();
    let mut next_retry = Instant::now();
    let mut next_sink = Instant::now();
    let mut sink = None;
    loop {
        ticker.tick().await;
        let now = Instant::now();
        if now >= next_status {
            let raw = timeout(RPC_TIMEOUT, proxy.get_status()).await??;
            let status: Status = serde_json::from_str(&raw)?;
            active = status.effect == "audio";
            feed.lock().expect("audio feed").response = status.rgb_speed.clamp(1, 10);
            next_status = now + Duration::from_millis(500);
            if !active {
                // Joining the old reader prevents a late EOF from overwriting
                // the idle status or the next capture's first frame.
                capture = None;
                next_retry = now;
                feed.lock().expect("audio feed").clear("Audio mode is off");
            }
        }
        if !active {
            continue;
        }
        if now >= next_sink {
            let current = default_sink().await;
            if current.is_some() && sink.is_some() && current != sink {
                capture = None;
                next_retry = now;
            }
            sink = current;
            next_sink = now + Duration::from_secs(2);
        }
        if capture
            .as_ref()
            .is_some_and(|capture| !capture.is_running())
        {
            capture = None;
        }
        if capture.is_none() && now >= next_retry {
            capture = Some(AudioCapture::start(Arc::clone(feed)));
            next_retry = now + Duration::from_secs(2);
        }
        let [bass, low, mid, high] = feed.lock().expect("audio feed").snapshot().levels;
        timeout(RPC_TIMEOUT, proxy.set_audio_levels(bass, low, mid, high)).await??;
    }
}
