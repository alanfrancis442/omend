// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

mod engine;
mod iface;
mod ownership;
#[cfg(test)]
mod tests;

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use omen_common::{Config, SERVICE_NAME, SERVICE_PATH, Sensors};
use tokio::time::MissedTickBehavior;
use tracing::info;
use zbus::connection::Builder;

use crate::engine::Engine;
use crate::iface::OmenIface;

#[derive(Debug, Parser)]
#[command(
    name = "omend",
    about = "Hardware daemon for HP OMEN and Victus laptops"
)]
struct Args {
    /// Read and write this config instead of /etc/omend/config.toml.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Read-only session-bus daemon. Implied when not running as root.
    #[arg(long, conflicts_with = "system")]
    session: bool,
    /// Bind the system bus. Requires the D-Bus policy and root.
    #[arg(long)]
    system: bool,
    /// Request the configured fan recovery once and exit, without serving
    /// D-Bus. For the unit's ExecStopPost after a crash or SIGKILL. Requires
    /// root and refuses to act while another daemon holds the hardware lock.
    #[arg(long, conflicts_with = "session")]
    recover_fans: bool,
}

const SYSTEM_CONFIG: &str = "/etc/omend/config.toml";
const OWNERSHIP_LOCK: &str = "/run/omend.lock";

/// The root daemon never falls back to a configuration in the working
/// directory: that file may be controlled by an unprivileged user.
fn config_for(root: bool, explicit: Option<&Path>) -> (Config, Option<PathBuf>) {
    match explicit {
        Some(path) => Config::resolve(Some(path)),
        None if root => Config::resolve(Some(Path::new(SYSTEM_CONFIG))),
        None => Config::resolve(None),
    }
}

/// Settings in a root daemon's configuration grant hardware control. Report
/// files or directories that someone other than root can modify.
fn config_permission_warnings(path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    for (label, target) in [("configuration", path), ("configuration directory", parent)] {
        let Ok(metadata) = std::fs::symlink_metadata(target) else {
            continue;
        };
        if metadata.uid() != 0 {
            warnings.push(format!(
                "{label} {} is owned by uid {}, not root",
                target.display(),
                metadata.uid()
            ));
        }
        if metadata.mode() & 0o022 != 0 {
            warnings.push(format!(
                "{label} {} is writable by group or others",
                target.display()
            ));
        }
    }
    warnings
}

/// One-shot fan recovery with the configured recovery mode.
fn recover_fans_once(config: &Config, sensors: Sensors) -> Result<(), String> {
    let sensors = sensors.with_emergency_max_recovery(config.thermal.experimental_emergency_max);
    if !sensors.fan_recovery_available() {
        return Err("pwm1_enable is not present; nothing to recover".into());
    }
    sensors.recover_fans()
}

fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let rest = line.strip_prefix("Uid:")?;
                rest.split_whitespace().nth(1)?.parse::<u32>().ok()
            })
        })
        == Some(0)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "omend=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = is_root();
    if args.recover_fans {
        anyhow::ensure!(root, "fan recovery must run as root");
        // A live daemon owns the fans; never fight it.
        let _ownership = ownership::acquire(Path::new(OWNERSHIP_LOCK))?;
        let (config, _) = config_for(true, args.config.as_deref());
        let recovery = Sensors::discover();
        let name = recovery.fan_recovery_name();
        recover_fans_once(&config, recovery)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("request {name}"))?;
        info!("requested {name}; cooling unverified");
        return Ok(());
    }
    anyhow::ensure!(
        !(root && args.session),
        "root must use the system bus; session daemons are read-only"
    );
    anyhow::ensure!(!args.system || root, "the system daemon must run as root");
    let use_system = root;
    // Hold this across initialization, shutdown and interface destruction. Never
    // unlink it: a replacement inode would allow two independent lock holders.
    let _ownership = if root {
        Some(ownership::acquire(Path::new(OWNERSHIP_LOCK))?)
    } else {
        None
    };
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("register SIGTERM handler")?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .context("register SIGINT handler")?;
    let (config, config_path) = config_for(root, args.config.as_deref());
    if root && let Some(path) = &config_path {
        for warning in config_permission_warnings(path) {
            tracing::warn!("{warning}; non-root writers can change hardware settings");
        }
    }
    let sensors = Sensors::discover();
    let sensors = if root { sensors } else { sensors.read_only() };
    let report = sensors.report();
    info!("{}", report.format().replace('\n', " | "));

    let poll = config.poll();
    let engine = Arc::new(Mutex::new(Engine::new(config, config_path, sensors)));
    let iface = OmenIface {
        engine: Arc::clone(&engine),
    };
    let connection = serve(iface, use_system).await?;
    engine.lock().expect("engine lock").start();
    info!(
        bus = if use_system { "system" } else { "session" },
        name = SERVICE_NAME,
        path = SERVICE_PATH,
        "omend is listening"
    );

    let background = Arc::clone(&engine);
    let mut worker = tokio::spawn(async move {
        let mut fans = tokio::time::interval(poll);
        let mut lights = tokio::time::interval(Duration::from_millis(100));
        fans.set_missed_tick_behavior(MissedTickBehavior::Delay);
        lights.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = fans.tick() => {
                    background.lock().expect("hardware engine lock poisoned").poll();
                }
                _ = lights.tick() => {
                    background.lock().expect("hardware engine lock poisoned").animate();
                }
            }
        }
    });

    let worker_failed = tokio::select! {
        _ = terminate.recv() => {
            info!("received SIGTERM; requesting configured fan recovery");
            false
        },
        _ = interrupt.recv() => {
            info!("received SIGINT; requesting configured fan recovery");
            false
        },
        result = &mut worker => {
            tracing::error!(?result, "hardware worker stopped; requesting configured fan recovery");
            true
        },
    };
    worker.abort();
    if !worker_failed {
        let _ = worker.await;
    }
    // Recover a poisoned lock so even a failed request cannot skip fan recovery.
    let result = engine
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .shutdown();
    drop(connection);
    result
        .map_err(anyhow::Error::msg)
        .context("request configured fan recovery")?;
    anyhow::ensure!(!worker_failed, "hardware worker stopped unexpectedly");
    Ok(())
}

async fn serve(iface: OmenIface, system: bool) -> anyhow::Result<zbus::Connection> {
    let builder = if system {
        Builder::system()
    } else {
        Builder::session()
    }
    .context("connect to dbus")?;
    serve_on(builder, iface).await
}

async fn serve_on(builder: Builder<'_>, iface: OmenIface) -> anyhow::Result<zbus::Connection> {
    let connection = builder
        .serve_at(SERVICE_PATH, iface)
        .context("serve the omend object")?
        .build()
        .await
        .context("start the dbus connection")?;
    // Do not rely on Builder::name: the locked zbus version can queue a name
    // request and return a live connection before becoming its primary owner.
    let reply = connection
        .request_name_with_flags(SERVICE_NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        .await
        .context("exclusively acquire the omend bus name")?;
    anyhow::ensure!(
        reply == zbus::fdo::RequestNameReply::PrimaryOwner,
        "omend did not become the primary bus owner"
    );
    Ok(connection)
}
