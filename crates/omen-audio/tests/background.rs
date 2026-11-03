// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use omen_audio::{AudioFeed, AudioProxy};
use omen_common::Status;

struct ChildGuard(Child, bool);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.1 {
            let _ = Command::new("kill")
                .args(["-TERM", &self.0.id().to_string()])
                .status();
            for _ in 0..100 {
                if matches!(self.0.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct MockOmend {
    status: Arc<Mutex<Status>>,
    packets: Arc<AtomicU64>,
}
#[zbus::interface(name = "dev.omend")]
impl MockOmend {
    fn get_status(&self) -> String {
        serde_json::to_string(&*self.status.lock().unwrap()).unwrap()
    }
    fn set_audio_levels(&self, _bass: u8, _low: u8, _mid: u8, _high: u8) {
        self.packets.fetch_add(1, Ordering::Relaxed);
    }
}

async fn wait_snapshot(
    proxy: &AudioProxy<'_>,
    predicate: impl Fn(&AudioFeed) -> bool,
) -> AudioFeed {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(Ok(raw)) =
                tokio::time::timeout(Duration::from_millis(500), proxy.get_snapshot()).await
            {
                let snapshot: AudioFeed = serde_json::from_str(&raw).unwrap();
                if predicate(&snapshot) {
                    return snapshot;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("background service did not publish the expected frame")
}

/// A private bus, simulated speakers and a fake hardware daemon keep this test
/// independent of the user's real session, microphone and keyboard.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audio_continues_without_gui_clients_and_capture_follows_daemon_mode() {
    let directory = TestDirectory(
        std::env::temp_dir().join(format!("omen-audio-integration-{}", std::process::id())),
    );
    fs::create_dir_all(&directory.0).unwrap();
    let mut bus = ChildGuard(
        Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1", "--nopidfile"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required for this integration test"),
        false,
    );
    let mut address = String::new();
    BufReader::new(bus.0.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let address = address.trim();
    assert!(!address.is_empty(), "private D-Bus failed to start");
    let mut status = Status::offline("test");
    status.daemon_ok = true;
    status.effect = "audio".into();
    let status = Arc::new(Mutex::new(status));
    let packets = Arc::new(AtomicU64::new(0));
    let _hardware = zbus::connection::Builder::address(address)
        .unwrap()
        .name("dev.omend")
        .unwrap()
        .serve_at(
            "/dev/omend",
            MockOmend {
                status: Arc::clone(&status),
                packets: Arc::clone(&packets),
            },
        )
        .unwrap()
        .build()
        .await
        .unwrap();

    let pcm = directory.0.join("speaker.pcm");
    let mut samples = Vec::new();
    for index in 0..512 {
        let sample =
            (0.4 * (std::f32::consts::TAU * 87.0 * index as f32 / 22050.0).sin()).to_le_bytes();
        samples.extend_from_slice(&sample);
        samples.extend_from_slice(&sample);
    }
    fs::write(&pcm, samples).unwrap();
    let parec = directory.0.join("parec");
    fs::write(
        &parec,
        format!(
            "#!/bin/sh\nwhile :; do cat '{}'; sleep 0.02; done\n",
            pcm.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&parec, fs::Permissions::from_mode(0o755)).unwrap();
    let pactl = directory.0.join("pactl");
    fs::write(&pactl, "#!/bin/sh\nprintf 'test-speakers\\n'\n").unwrap();
    fs::set_permissions(&pactl, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        directory.0.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut service = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_omen-audio"))
            .env("DBUS_SESSION_BUS_ADDRESS", address)
            .env("DBUS_SYSTEM_BUS_ADDRESS", address)
            .env("PATH", path)
            .spawn()
            .unwrap(),
        true,
    );
    let gui = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    let proxy = AudioProxy::new(&gui).await.unwrap();
    wait_snapshot(&proxy, |frame| frame.levels[0] > 0).await;
    drop(proxy);
    drop(gui);
    let before = packets.load(Ordering::Relaxed);
    tokio::time::timeout(Duration::from_secs(2), async {
        while packets.load(Ordering::Relaxed) < before + 4 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("audio forwarding stopped after the GUI disconnected");
    assert!(service.0.try_wait().unwrap().is_none());

    // Reopening a GUI observes the existing producer and does not spawn one.
    let gui = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    let proxy = AudioProxy::new(&gui).await.unwrap();
    status.lock().unwrap().rgb_speed = 9;
    wait_snapshot(&proxy, |frame| frame.response == 9 && frame.levels[0] > 0).await;
    status.lock().unwrap().effect = "static".into();
    wait_snapshot(&proxy, |frame| {
        frame.note == "Audio mode is off" && frame.levels == [0; 4]
    })
    .await;
    let before = packets.load(Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        packets.load(Ordering::Relaxed),
        before,
        "inactive audio must not keep writing hardware frames"
    );
    status.lock().unwrap().effect = "audio".into();
    wait_snapshot(&proxy, |frame| frame.levels[0] > 0).await;
}
