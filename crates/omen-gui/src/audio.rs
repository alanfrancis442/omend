// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use omen_audio::AudioFeed;
use omen_audio::AudioProxy;

/// Read the user service's spectrum. Dropping this client never stops capture
/// or sends a final zero frame to the hardware daemon.
pub async fn watch(feed: Arc<Mutex<AudioFeed>>) {
    loop {
        if let Err(err) = follow(&feed).await {
            feed.lock().expect("audio feed").clear(format!(
                "Background audio unavailable. Start omen-audio.service: {err}"
            ));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn follow(feed: &Mutex<AudioFeed>) -> Result<(), String> {
    // Use the same session bus as the unprivileged audio producer, independently
    // of the system/session bus used for hardware control.
    let connection = tokio::time::timeout(Duration::from_secs(1), omen_audio::session_connection())
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    let proxy = AudioProxy::new(&connection)
        .await
        .map_err(|err| err.to_string())?;
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let raw = tokio::time::timeout(Duration::from_millis(250), proxy.get_snapshot())
            .await
            .map_err(|err| err.to_string())?
            .map_err(|err| err.to_string())?;
        let snapshot = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
        *feed.lock().expect("audio feed") = snapshot;
    }
}
