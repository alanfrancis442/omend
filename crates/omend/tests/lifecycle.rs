// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Real-process signal checks use only an unprivileged read-only session daemon.
//! Fake-sysfs unit tests separately verify the fan restoration operation.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use omen_common::OmendProxy;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn sigterm_and_sigint_exit_cleanly_and_release_the_bus_name() {
    // A root session daemon is deliberately forbidden. CI must run this test
    // as a normal user; running as root still verifies that rejection instead.
    if fs::metadata("/proc/self").unwrap().uid() == 0 {
        let output = Command::new(env!("CARGO_BIN_EXE_omend"))
            .arg("--session")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("root must use the system bus"));
        return;
    }
    let directory =
        Directory(std::env::temp_dir().join(format!("omend-lifecycle-{}", std::process::id())));
    fs::create_dir(&directory.0).unwrap();
    fs::write(
        directory.0.join("config.toml"),
        include_str!("../../../config/omend.toml"),
    )
    .unwrap();
    let mut bus = ChildGuard(
        Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .arg(format!("--address=unix:tmpdir={}", directory.0.display()))
            .env("XDG_RUNTIME_DIR", &directory.0)
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required for lifecycle tests"),
    );
    let mut address = String::new();
    BufReader::new(bus.0.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    assert!(!address.trim().is_empty(), "private D-Bus failed to start");
    let client = zbus::connection::Builder::address(address.trim())
        .unwrap()
        .build()
        .await
        .unwrap();
    let dbus = zbus::fdo::DBusProxy::new(&client).await.unwrap();
    let proxy = OmendProxy::new(&client).await.unwrap();

    for signal in ["-TERM", "-INT"] {
        let log = fs::File::create(directory.0.join("daemon.log")).unwrap();
        let mut daemon = ChildGuard(
            Command::new(env!("CARGO_BIN_EXE_omend"))
                .args(["--session", "--config"])
                .arg(directory.0.join("config.toml"))
                .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
                .env("DBUS_SYSTEM_BUS_ADDRESS", address.trim())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if proxy.get_status().await.is_ok() {
                    break;
                }
                assert!(
                    daemon.0.try_wait().unwrap().is_none(),
                    "daemon exited before readiness"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("read-only session daemon did not start");
        assert!(
            Command::new("kill")
                .args([signal, &daemon.0.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let status = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(status) = daemon.0.try_wait().unwrap() {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("daemon did not exit after signal");
        assert!(
            status.success(),
            "{signal}: {}",
            fs::read_to_string(directory.0.join("daemon.log")).unwrap()
        );
        assert!(
            !dbus
                .name_has_owner(omen_common::SERVICE_NAME.try_into().unwrap())
                .await
                .unwrap()
        );
    }
}
