// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use omen_common::{OmendProxy, Preset};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "omend-safety-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let fixture = Self(directory);
        fixture.write("class/hwmon/hp/name", "hp");
        fixture.write("class/hwmon/hp/pwm1", "77");
        fixture.write("class/hwmon/hp/pwm1_enable", "2");
        fixture.write("class/hwmon/cpu/name", "k10temp");
        fixture.write("class/hwmon/cpu/temp1_input", "60000");
        fixture.write("class/hwmon/gpu/name", "amdgpu");
        fixture.write("class/hwmon/gpu/temp1_input", "50000");
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join(path)).unwrap().trim().into()
    }

    fn engine(&self) -> Engine {
        self.engine_with_recovery(false)
    }

    fn engine_with_recovery(&self, emergency_max: bool) -> Engine {
        let mut config = Config::default();
        // Recovery tests must not pass merely because a periodic rewrite is due.
        config.thermal.watchdog_secs = 3600;
        config.thermal.experimental_emergency_max = emergency_max;
        let path = self.0.join("config.toml");
        config.save(&path).unwrap();
        Engine::new(config, Some(path), Sensors::discover_in(&self.0))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn save_failure_is_reported_without_hiding_applied_runtime_state_or_corrupting_disk() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    fixture.write("config.toml", "[malformed");
    let error = engine.set_preset("max").unwrap_err();
    assert!(error.contains("not confirmed saved"));
    assert!(error.contains("runtime state or hardware may already have changed"));
    assert!(error.contains("malformed"));
    assert_eq!(fixture.read("config.toml"), "[malformed");
    assert_eq!(engine.status().preset, "max");
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "255");
    engine.poll();
    assert!(engine.status().message.contains("not confirmed saved"));
    // Explicit repair permits the next save and clears only the save warning.
    Config::default()
        .save(&fixture.0.join("repaired.toml"))
        .unwrap();
    fs::rename(
        fixture.0.join("repaired.toml"),
        fixture.0.join("config.toml"),
    )
    .unwrap();
    engine.set_hysteresis(7).unwrap();
    assert!(!engine.status().message.contains("not confirmed saved"));
    let loaded = Config::load(&fixture.0.join("config.toml")).unwrap();
    assert_eq!(loaded.thermal.preset, Preset::Max);
    assert_eq!(loaded.thermal.hysteresis_secs, 7);
}

#[test]
fn absent_save_destination_is_not_acknowledged_as_persistent() {
    let fixture = Fixture::new();
    let mut engine = Engine::new(Config::default(), None, Sensors::discover_in(&fixture.0));
    engine.start();
    let error = engine.set_hysteresis(9).unwrap_err();
    assert!(error.contains("no configuration destination"));
    assert_eq!(engine.status().hysteresis_secs, 9);
    assert!(engine.status().message.contains("not confirmed saved"));
}

#[test]
fn no_writes_before_activation_or_after_shutdown_and_preset_is_preserved() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.poll();
    engine.animate();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "77");
    engine.start();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "1");
    engine.shutdown().unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    assert_eq!(engine.status().preset, "balanced");
    engine.poll();
    engine.animate();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    assert!(!engine.is_running());
}

#[test]
fn dropping_active_engine_restores_firmware_control() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "1");
    drop(engine);
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
}

#[test]
fn sensor_loss_bypasses_spindown_delay_and_recovery_reapplies_selected_curve() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    // A surviving GPU reading must not hide the failure of a known CPU sensor.
    fixture.write("class/hwmon/cpu/temp1_input", "unreadable");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    assert!(
        engine
            .status()
            .message
            .contains("temperature sensor unavailable")
    );
    fixture.write("class/hwmon/cpu/temp1_input", "60000");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "1");
    assert!(
        !engine
            .status()
            .message
            .contains("temperature sensor unavailable")
    );
}

#[test]
fn implausible_sensor_readings_restore_firmware_control() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    for value in ["0", "-1000", "2147483647", "141000"] {
        fixture.write("class/hwmon/cpu/temp1_input", value);
        engine.poll();
        assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    }
}

#[test]
fn failed_pwm_write_recovers_even_when_pwm_file_is_no_longer_usable() {
    let fixture = Fixture::new();
    let sensors = Sensors::discover_in(&fixture.0);
    fs::remove_file(fixture.0.join("class/hwmon/hp/pwm1")).unwrap();
    fs::create_dir(fixture.0.join("class/hwmon/hp/pwm1")).unwrap();
    let error = sensors.apply_fan(true, 100).unwrap_err();
    assert!(error.contains("requested firmware Auto"));
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    sensors.apply_fan(false, 0).unwrap();
}

#[test]
fn shutdown_failure_is_reported() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    fs::remove_file(fixture.0.join("class/hwmon/hp/pwm1_enable")).unwrap();
    fs::create_dir(fixture.0.join("class/hwmon/hp/pwm1_enable")).unwrap();
    assert!(engine.shutdown().is_err());
    assert!(!engine.is_running());
}

#[test]
fn firmware_mode_reset_is_corrected_on_the_next_poll() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    let requested = fixture.read("class/hwmon/hp/pwm1");
    // Model a firmware reset/resume without waiting for the rewrite timer.
    fixture.write("class/hwmon/hp/pwm1_enable", "2");
    fixture.write("class/hwmon/hp/pwm1", "0");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "1");
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), requested);
}

#[test]
fn manual_pwm_readback_is_not_assumed_to_be_requested_duty() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    // hp-wmi may report scaled actual RPM while the fans are accelerating.
    fixture.write("class/hwmon/hp/pwm1", "1");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "1");
}

#[test]
fn firmware_auto_does_not_force_a_fixed_pwm() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    engine.set_preset("auto").unwrap();
    fixture.write("class/hwmon/hp/pwm1", "123");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "123");
}

#[test]
fn sensor_failure_reports_failed_firmware_recovery_and_retries() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    fixture.write("class/hwmon/cpu/temp1_input", "unreadable");
    let enable = fixture.0.join("class/hwmon/hp/pwm1_enable");
    fs::remove_file(&enable).unwrap();
    fs::create_dir(&enable).unwrap();
    engine.poll();
    assert!(
        engine
            .status()
            .message
            .contains("firmware Auto recovery request failed")
    );
    fs::remove_dir(&enable).unwrap();
    fixture.write("class/hwmon/hp/pwm1_enable", "1");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    assert!(engine.status().message.contains("requested firmware Auto"));
}

#[test]
fn failed_manual_mode_write_reports_both_failures() {
    let fixture = Fixture::new();
    let sensors = Sensors::discover_in(&fixture.0);
    let enable = fixture.0.join("class/hwmon/hp/pwm1_enable");
    fs::remove_file(&enable).unwrap();
    fs::create_dir(&enable).unwrap();
    let error = sensors.apply_fan(true, 255).unwrap_err();
    assert!(error.contains("firmware Auto recovery request also failed"));
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "77");
}

#[test]
fn failed_rewrite_is_retried_even_after_mode_readback_recovers() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.start();
    let requested = fixture.read("class/hwmon/hp/pwm1");
    let enable = fixture.0.join("class/hwmon/hp/pwm1_enable");
    fs::remove_file(&enable).unwrap();
    fs::create_dir(&enable).unwrap();
    engine.poll();
    assert!(
        engine
            .status()
            .message
            .contains("firmware Auto recovery request also failed")
    );
    fs::remove_dir(&enable).unwrap();
    fixture.write("class/hwmon/hp/pwm1_enable", "1");
    fixture.write("class/hwmon/hp/pwm1", "1");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), requested);
    assert!(
        !engine
            .status()
            .message
            .contains("firmware Auto recovery request also failed")
    );
}

#[test]
fn one_shot_recovery_uses_the_configured_mode_without_a_running_engine() {
    let fixture = Fixture::new();
    fixture.write("class/hwmon/hp/pwm1_enable", "1");
    let mut config = Config::default();
    recover_fans_once(&config, Sensors::discover_in(&fixture.0)).unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    config.thermal.experimental_emergency_max = true;
    recover_fans_once(&config, Sensors::discover_in(&fixture.0)).unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
    fs::remove_file(fixture.0.join("class/hwmon/hp/pwm1_enable")).unwrap();
    assert!(recover_fans_once(&config, Sensors::discover_in(&fixture.0)).is_err());
}

#[test]
fn root_configuration_never_comes_from_the_working_directory() {
    let fixture = Fixture::new();
    let explicit = fixture.0.join("explicit.toml");
    assert_eq!(config_for(true, Some(&explicit)).1, Some(explicit.clone()));
    assert_eq!(
        config_for(true, None).1,
        Some(PathBuf::from("/etc/omend/config.toml"))
    );
}

#[test]
fn modifiable_root_configuration_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let path = fixture.0.join("config.toml");
    Config::default().save(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    let warnings = config_permission_warnings(&path);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("writable by group or others"))
    );
    if fs::metadata(&path).unwrap().uid() != 0 {
        assert!(warnings.iter().any(|w| w.contains("not root")));
    }
}

#[test]
fn emergency_max_needs_no_pwm_data_node() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("class/hwmon/hp/pwm1")).unwrap();
    let sensors = Sensors::discover_in(&fixture.0);
    assert!(!sensors.fan_writable);
    assert!(sensors.fan_recovery_available());
    sensors.request_emergency_max().unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
    assert!(!fixture.0.join("class/hwmon/hp/pwm1").exists());
}

#[test]
fn experimental_sensor_failure_requests_max_then_recovers_selected_curve() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine_with_recovery(true);
    engine.start();
    let pwm = fixture.read("class/hwmon/hp/pwm1");
    fixture.write("class/hwmon/cpu/temp1_input", "unreadable");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), pwm);
    assert!(
        engine
            .status()
            .message
            .contains("requested experimental maximum-fan mode")
    );
    assert!(engine.status().message.contains("cooling unverified"));
    fixture.write("class/hwmon/cpu/temp1_input", "60000");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "1");
    assert_eq!(engine.status().preset, "balanced");
    assert!(
        !engine
            .status()
            .message
            .contains("temperature sensor unavailable")
    );
}

#[test]
fn experimental_shutdown_and_drop_preserve_config_and_request_max() {
    let fixture = Fixture::new();
    let mut config = Config::default();
    config.thermal.experimental_emergency_max = true;
    let config_path = fixture.0.join("config.toml");
    config.save(&config_path).unwrap();
    let original = fs::read(&config_path).unwrap();
    let mut engine = Engine::new(
        config,
        Some(config_path.clone()),
        Sensors::discover_in(&fixture.0),
    );
    engine.start();
    engine.shutdown().unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
    assert_eq!(engine.status().preset, "balanced");
    fixture.write("class/hwmon/hp/pwm1_enable", "2");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    engine.start();
    drop(engine);
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
    assert_eq!(fs::read(config_path).unwrap(), original);
}

#[test]
fn experimental_shutdown_requests_max_even_without_manual_pwm_support() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("class/hwmon/hp/pwm1")).unwrap();
    let mut engine = fixture.engine_with_recovery(true);
    engine.start();
    engine.shutdown().unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
}

#[test]
fn experimental_failed_pwm_write_requests_max_without_using_pwm_data() {
    let fixture = Fixture::new();
    let sensors = Sensors::discover_in(&fixture.0).with_emergency_max_recovery(true);
    fs::remove_file(fixture.0.join("class/hwmon/hp/pwm1")).unwrap();
    fs::create_dir(fixture.0.join("class/hwmon/hp/pwm1")).unwrap();
    let error = sensors.apply_fan(true, 100).unwrap_err();
    assert!(error.contains("requested experimental maximum-fan mode; cooling unverified"));
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
}

#[test]
fn missing_manual_pwm_requests_experimental_max_without_entering_manual() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("class/hwmon/hp/pwm1")).unwrap();
    let sensors = Sensors::discover_in(&fixture.0).with_emergency_max_recovery(true);
    assert!(
        sensors
            .apply_fan(true, 100)
            .unwrap_err()
            .contains("pwm1 is not present")
    );
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
}

#[test]
fn experimental_failed_max_request_reports_error_and_retries() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine_with_recovery(true);
    engine.start();
    fixture.write("class/hwmon/cpu/temp1_input", "unreadable");
    let enable = fixture.0.join("class/hwmon/hp/pwm1_enable");
    fs::remove_file(&enable).unwrap();
    fs::create_dir(&enable).unwrap();
    engine.poll();
    assert!(
        engine
            .status()
            .message
            .contains("maximum-fan mode recovery request failed")
    );
    fs::remove_dir(&enable).unwrap();
    fixture.write("class/hwmon/hp/pwm1_enable", "1");
    engine.poll();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
    assert!(
        engine
            .status()
            .message
            .contains("requested experimental maximum-fan mode")
    );
    fs::remove_file(&enable).unwrap();
    fs::create_dir(&enable).unwrap();
    assert!(engine.shutdown().is_err());
    assert!(!engine.is_running());
}

#[test]
fn experimental_manual_and_recovery_errors_are_both_reported() {
    let fixture = Fixture::new();
    let sensors = Sensors::discover_in(&fixture.0).with_emergency_max_recovery(true);
    let enable = fixture.0.join("class/hwmon/hp/pwm1_enable");
    fs::remove_file(&enable).unwrap();
    fs::create_dir(&enable).unwrap();
    let error = sensors.apply_fan(true, 255).unwrap_err();
    assert!(error.contains("maximum-fan mode recovery request also failed"));
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "77");
}

#[test]
fn explicit_auto_is_independent_of_experimental_recovery() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine_with_recovery(true);
    engine.start();
    engine.set_preset("auto").unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    engine.shutdown().unwrap();
    assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "0");
}

#[test]
fn read_only_session_cannot_write_fans_profiles_or_lights() {
    let fixture = Fixture::new();
    fixture.write("firmware/acpi/platform_profile", "balanced");
    fixture.write("class/leds/hp::kbd_backlight/brightness", "123");
    let sensors = Sensors::discover_in(&fixture.0).read_only();
    assert!(!sensors.fan_writable);
    assert!(!sensors.fan_recovery_available());
    assert!(sensors.request_emergency_max().is_err());
    assert!(
        sensors
            .clone()
            .with_emergency_max_recovery(true)
            .recover_fans()
            .is_err()
    );
    assert!(!sensors.profile_writable());
    assert!(sensors.apply_fan(true, 255).is_err());
    assert!(sensors.apply_profile("performance").is_err());
    assert!(
        sensors
            .apply_rgb(&[(
                omen_common::ZoneId::All,
                omen_common::RgbColor::new(1, 2, 3),
                255
            )])
            .is_err()
    );
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "77");
    assert_eq!(fixture.read("firmware/acpi/platform_profile"), "balanced");
    assert_eq!(
        fixture.read("class/leds/hp::kbd_backlight/brightness"),
        "123"
    );
}

struct TestBus(Child, String);

impl TestBus {
    fn new(fixture: &Fixture, role: &str) -> Self {
        // Map the test UID/GID to one production role at a time. No host users,
        // groups, policies or bus instances are modified. The baseline allows
        // owning names so the same-UID mock server can export the real interface.
        let metadata = fs::metadata("/proc/self").unwrap();
        let uid = if role == "root" {
            metadata.uid().to_string()
        } else {
            "4294967294".into()
        };
        let gid = if role == "group" {
            metadata.gid().to_string()
        } else {
            "4294967294".into()
        };
        let policy = include_str!("../../../packaging/dev.omend.conf")
            .replace("user=\"root\"", &format!("user=\"{uid}\""))
            .replace("group=\"omen\"", &format!("group=\"{gid}\""));
        fixture.write("policy.conf", &policy);
        fixture.write("bus.conf", &format!(r#"<busconfig>
            <type>session</type><listen>unix:tmpdir={}</listen><auth>EXTERNAL</auth>
            <policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy>
            <include>{}/policy.conf</include>
            </busconfig>"#, fixture.0.display(), fixture.0.display()));
        let child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}/bus.conf", fixture.0.display()))
            .args(["--nofork", "--print-address=1", "--nopidfile"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required for policy tests");
        let mut bus = Self(child, String::new());
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut bus.1)
            .unwrap();
        bus.1 = bus.1.trim().into();
        assert!(!bus.1.is_empty(), "private test bus failed to start");
        bus
    }

    async fn connect(&self) -> zbus::Connection {
        Builder::address(self.1.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn export(bus: &TestBus, engine: Arc<Mutex<Engine>>) -> zbus::Connection {
    serve_on(
        Builder::address(bus.1.as_str()).unwrap(),
        OmenIface { engine },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn dbus_reports_unsaved_changes_and_retains_warning_until_successful_save() {
    let fixture = Fixture::new();
    let bus = TestBus::new(&fixture, "root");
    let engine = Arc::new(Mutex::new(fixture.engine()));
    let _server = export(&bus, Arc::clone(&engine)).await;
    engine.lock().unwrap().start();
    fixture.write("config.toml", "[malformed");
    let client = bus.connect().await;
    let proxy = OmendProxy::new(&client).await.unwrap();
    let error = proxy.set_preset("max").await.unwrap_err();
    match error {
        zbus::Error::MethodError(name, Some(message), _) => {
            assert_eq!(name.as_str(), "org.freedesktop.DBus.Error.Failed");
            assert!(message.contains("not confirmed saved"));
            assert!(message.contains("runtime state or hardware may already have changed"));
        }
        error => panic!("expected an explicit persistence error, received {error}"),
    }
    assert_eq!(fixture.read("config.toml"), "[malformed");
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "255");
    let status = proxy.get_status().await.unwrap();
    assert!(status.contains("not confirmed saved"));
    assert!(status.contains("max"));
    engine.lock().unwrap().shutdown().unwrap();
}

#[tokio::test]
async fn default_policy_denies_every_mutator_but_allows_status() {
    let fixture = Fixture::new();
    let bus = TestBus::new(&fixture, "untrusted");
    let engine = Arc::new(Mutex::new(fixture.engine()));
    let _server = export(&bus, Arc::clone(&engine)).await;
    engine.lock().unwrap().start();
    let before = fixture.read("class/hwmon/hp/pwm1");
    let client = bus.connect().await;
    let proxy = OmendProxy::new(&client).await.unwrap();
    assert!(proxy.get_status().await.is_ok());
    assert!(proxy.get_temperatures().await.is_ok());
    assert!(proxy.get_fan_speeds().await.is_ok());
    let results = [
        proxy.set_preset("max").await,
        proxy.set_fan_curve(vec![(40, 70), (90, 255)]).await,
        proxy
            .save_fan_curve("test", vec![(40, 70), (90, 255)], 4)
            .await,
        proxy.delete_fan_curve("test").await,
        proxy.set_power_curve("ac", "max").await,
        proxy.set_hysteresis(5).await,
        proxy.set_rgb_zone(0, 1, 2, 3, 100).await,
        proxy.set_rgb_brightness(100).await,
        proxy.set_rgb_effect("static").await,
        proxy.set_rgb_style(5, 80, 100, false).await,
        proxy.set_audio_levels(1, 2, 3, 4).await,
        proxy
            .set_custom_effect("cycle", vec![(1, 2, 3), (4, 5, 6)])
            .await,
        proxy.set_platform_profile("performance").await,
        proxy.reapply_state().await,
        // Omitting the interface must not bypass the policy.
        client
            .call_method(
                Some(SERVICE_NAME),
                SERVICE_PATH,
                None::<&str>,
                "SetPreset",
                &("max",),
            )
            .await
            .map(|_| ()),
    ];
    for result in results {
        match result.unwrap_err() {
            zbus::Error::MethodError(name, _, _) => {
                assert_eq!(name.as_str(), "org.freedesktop.DBus.Error.AccessDenied")
            }
            error => panic!("expected a policy denial, received {error}"),
        }
    }
    assert_eq!(fixture.read("class/hwmon/hp/pwm1"), before);
    assert_eq!(engine.lock().unwrap().status().preset, "balanced");
    engine.lock().unwrap().shutdown().unwrap();
}

#[tokio::test]
async fn root_and_explicit_group_can_control_hardware_and_stopped_interface_rejects_calls() {
    for role in ["root", "group"] {
        let fixture = Fixture::new();
        let bus = TestBus::new(&fixture, role);
        let engine = Arc::new(Mutex::new(fixture.engine()));
        let _server = export(&bus, Arc::clone(&engine)).await;
        let client = bus.connect().await;
        let proxy = OmendProxy::new(&client).await.unwrap();
        assert!(proxy.set_preset("max").await.is_err());
        assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "77");
        engine.lock().unwrap().start();
        proxy.set_preset("max").await.unwrap();
        assert_eq!(fixture.read("class/hwmon/hp/pwm1"), "255");
        engine.lock().unwrap().shutdown().unwrap();
        assert!(proxy.set_preset("balanced").await.is_err());
        assert_eq!(fixture.read("class/hwmon/hp/pwm1_enable"), "2");
    }
}

#[tokio::test]
async fn second_bus_owner_is_rejected_before_activation() {
    let fixture = Fixture::new();
    let bus = TestBus::new(&fixture, "root");
    let first = Arc::new(Mutex::new(fixture.engine()));
    let _server = export(&bus, Arc::clone(&first)).await;
    first.lock().unwrap().start();
    let second_fixture = Fixture::new();
    let second = Arc::new(Mutex::new(second_fixture.engine()));
    let result = serve_on(
        Builder::address(bus.1.as_str()).unwrap(),
        OmenIface {
            engine: Arc::clone(&second),
        },
    )
    .await;
    assert!(result.is_err());
    assert!(!second.lock().unwrap().is_running());
    assert_eq!(second_fixture.read("class/hwmon/hp/pwm1"), "77");
    first.lock().unwrap().shutdown().unwrap();
}
