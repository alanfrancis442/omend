// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::io::{self, Write};

use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use omen_common::{OmendProxy, Profile, RgbColor, Sensors, Status, ZoneId, connect, waybar_json};

#[derive(Debug, Parser)]
#[command(
    name = "omen-cli",
    about = "Control an HP OMEN or Victus through the omend daemon"
)]
struct Cli {
    /// Talk to a session-bus daemon (`omend --session`) instead of the system bus.
    #[arg(long, global = true)]
    session: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print temperatures, fan speeds, and the active preset.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// JSON line for a Waybar custom module.
    Waybar,
    /// Fan preset, or a custom temperature-to-PWM curve.
    Fan {
        #[arg(long, value_name = "QUIET|BALANCED|MAX|CUSTOM|AUTO")]
        preset: Option<String>,
        /// Comma-separated temp:pwm points, for example 40:70,60:120,80:220.
        #[arg(long)]
        curve: Option<String>,
        /// Store `--curve` under this name.
        #[arg(long)]
        save: Option<String>,
        /// Remove a saved curve.
        #[arg(long)]
        delete: Option<String>,
        /// Preset or saved curve to run on wall power. Pass an empty string for manual.
        #[arg(long)]
        plugged_in: Option<String>,
        /// Preset or saved curve to run on battery. Pass an empty string for manual.
        #[arg(long)]
        on_battery: Option<String>,
    },
    /// Keyboard zone color and software effect.
    Rgb {
        #[arg(long, default_value = "all")]
        zone: String,
        /// RRGGBB or #RRGGBB.
        #[arg(long)]
        color: Option<String>,
        #[arg(long, default_value_t = 160)]
        brightness: u8,
        #[arg(long, value_name = "STATIC|BREATHING|WAVE|RAINBOW")]
        effect: Option<String>,
    },
    /// ACPI platform profile. `next` cycles low-power, balanced, performance.
    Profile { profile: String },
    /// Rewrite fan PWM and RGB after resume.
    Reapply,
    /// Inspect sysfs without a running daemon.
    Doctor,
    /// Print a shell completion script.
    Completions { shell: Shell },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match dispatch().await {
        Ok(0) => std::process::ExitCode::SUCCESS,
        Ok(code) => std::process::ExitCode::from(code),
        Err(err) => {
            eprintln!("Error: {err:?}");
            std::process::ExitCode::from(1)
        }
    }
}

async fn dispatch() -> anyhow::Result<u8> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor => {
            let report = Sensors::discover().report();
            println!("{}", report.format());
            Ok(if report.hp_hwmon.is_none() { 2 } else { 0 })
        }
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "omen-cli", &mut io::stdout());
            Ok(0)
        }
        other => {
            run(cli.session, other).await?;
            Ok(0)
        }
    }
}

async fn run(session: bool, command: Command) -> anyhow::Result<()> {
    let connection = connect(session).await.context("connect to dbus")?;
    let proxy = OmendProxy::new(&connection)
        .await
        .context("build the omend proxy")?;

    match command {
        Command::Status { json } => {
            let status = fetch_status(&proxy).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                print_status(&status);
            }
        }
        Command::Waybar => {
            let status = fetch_status(&proxy)
                .await
                .unwrap_or_else(|err| Status::offline(err.to_string()));
            println!("{}", waybar_json(&status));
        }
        Command::Fan {
            preset,
            curve,
            save,
            delete,
            plugged_in,
            on_battery,
        } => {
            if preset.is_none()
                && curve.is_none()
                && save.is_none()
                && delete.is_none()
                && plugged_in.is_none()
                && on_battery.is_none()
            {
                anyhow::bail!(
                    "pass --preset, --curve, --save, --delete, --plugged-in, or --on-battery"
                );
            }
            if let Some(curve) = &curve {
                let points = parse_curve(curve)?;
                if let Some(name) = &save {
                    let hysteresis = fetch_status(&proxy).await?.hysteresis_secs;
                    proxy
                        .save_fan_curve(name, points, hysteresis)
                        .await
                        .context("save fan curve")?;
                } else {
                    proxy.set_fan_curve(points).await.context("set fan curve")?;
                }
            } else if save.is_some() {
                anyhow::bail!("--save needs --curve");
            }
            if let Some(name) = delete {
                proxy
                    .delete_fan_curve(&name)
                    .await
                    .context("delete fan curve")?;
            }
            if let Some(name) = plugged_in {
                proxy
                    .set_power_curve("ac", &name)
                    .await
                    .context("set wall-power curve")?;
            }
            if let Some(name) = on_battery {
                proxy
                    .set_power_curve("battery", &name)
                    .await
                    .context("set battery curve")?;
            }
            if let Some(preset) = preset {
                proxy.set_preset(&preset).await.context("set fan preset")?;
            }
            print_status(&fetch_status(&proxy).await?);
        }
        Command::Rgb {
            zone,
            color,
            brightness,
            effect,
        } => {
            if color.is_none() && effect.is_none() {
                anyhow::bail!("pass --color or --effect");
            }
            if let Some(color) = color {
                let parsed =
                    RgbColor::parse_hex(&color).context("color must be RRGGBB or #RRGGBB")?;
                let zone_id = ZoneId::parse(&zone).context("unknown zone")? as u8;
                proxy
                    .set_rgb_zone(zone_id, parsed.r, parsed.g, parsed.b, brightness)
                    .await
                    .context("set rgb zone")?;
            }
            if let Some(effect) = effect {
                proxy
                    .set_rgb_effect(&effect)
                    .await
                    .context("set rgb effect")?;
            }
            print_status(&fetch_status(&proxy).await?);
        }
        Command::Profile { profile } => {
            let name = if profile == "next" {
                "next".to_string()
            } else {
                Profile::parse(&profile)
                    .context("profile must be low-power, balanced, performance, or next")?
                    .as_str()
                    .to_string()
            };
            proxy
                .set_platform_profile(&name)
                .await
                .context("set platform profile")?;
            print_status(&fetch_status(&proxy).await?);
        }
        Command::Reapply => {
            proxy.reapply_state().await.context("reapply state")?;
        }
        Command::Doctor | Command::Completions { .. } => unreachable!(),
    }
    Ok(())
}

async fn fetch_status(proxy: &OmendProxy<'_>) -> anyhow::Result<Status> {
    let raw = proxy.get_status().await.context(
        "omend is not running. Start it with `omend --session`, or as root for fan writes",
    )?;
    serde_json::from_str(&raw).context("decode daemon status")
}

fn print_status(status: &Status) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "CPU  {}°C", status.cpu_c);
    let _ = writeln!(out, "GPU  {}°C", status.gpu_c);
    let _ = writeln!(out, "Fan1 {} RPM", status.fan1_rpm);
    let _ = writeln!(out, "Fan2 {} RPM", status.fan2_rpm);
    let _ = writeln!(out, "PWM  {}", status.pwm);
    let _ = writeln!(out, "Preset  {}", status.preset);
    let _ = writeln!(out, "Profile {}", status.profile);
    let _ = writeln!(out, "Effect  {}", status.effect);
    let _ = writeln!(
        out,
        "Fans {}    RGB {}",
        if status.fan_control_available {
            "writable"
        } else {
            "read-only"
        },
        if status.rgb_available {
            "present"
        } else {
            "absent"
        }
    );
    if status.library {
        let wall = if status.plugged_in.is_empty() {
            "manual"
        } else {
            status.plugged_in.as_str()
        };
        let battery = if status.on_battery.is_empty() {
            "manual"
        } else {
            status.on_battery.as_str()
        };
        let _ = writeln!(
            out,
            "Power {}    wall {wall}    battery {battery}",
            status.power
        );
        if !status.saved.is_empty() {
            let names = status
                .saved
                .iter()
                .map(|saved| saved.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "Saved {names}");
        }
    }
    if !status.message.is_empty() {
        let _ = writeln!(out, "{}", status.message);
    }
}

fn parse_curve(text: &str) -> anyhow::Result<Vec<(u32, u32)>> {
    let mut points = Vec::new();
    for part in text.split(',') {
        let (temp, pwm) = part
            .split_once(':')
            .context("curve points look like 40:70,60:120")?;
        points.push((
            temp.trim().parse().context("temperature")?,
            pwm.trim().parse().context("pwm")?,
        ));
    }
    Ok(points)
}
