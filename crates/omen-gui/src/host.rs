// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

#[derive(Clone, Default)]
pub(crate) struct HostSnapshot {
    pub(crate) cpu: f32,
    pub(crate) gpu: f32,
    pub(crate) mem: f32,
    pub(crate) mem_detail: String,
    pub(crate) disk: f32,
    pub(crate) disk_detail: String,
    pub(crate) power: String,
}

// Sampling may wait on a filesystem or a child process. Never do it in a
// Slint timer: that timer shares the thread that presents scrolling frames.
pub(crate) struct HostMonitor {
    latest: Arc<Mutex<HostSnapshot>>,
    stop: mpsc::Sender<()>,
}

impl HostMonitor {
    pub(crate) fn start(refresh: Arc<AtomicU64>) -> Self {
        let mut meters = HostMeters::default();
        Self::spawn(refresh, move || meters.sample())
    }

    fn spawn(
        refresh: Arc<AtomicU64>,
        mut sample: impl FnMut() -> HostSnapshot + Send + 'static,
    ) -> Self {
        let latest = Arc::new(Mutex::new(HostSnapshot::default()));
        let output = Arc::clone(&latest);
        let (stop, stopped) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                // Only hold the shared lock to publish a finished sample.
                let snapshot = sample();
                *output.lock().expect("host snapshot") = snapshot;
                let interval = Duration::from_millis(refresh.load(Ordering::Relaxed).max(100));
                if stopped.recv_timeout(interval) != Err(mpsc::RecvTimeoutError::Timeout) {
                    break;
                }
            }
        });
        Self { latest, stop }
    }

    pub(crate) fn snapshot(&self) -> Option<HostSnapshot> {
        self.latest.try_lock().ok().map(|snapshot| snapshot.clone())
    }
}

impl Drop for HostMonitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

#[derive(Default)]
pub(crate) struct HostMeters {
    pub(crate) prev_idle: u64,
    pub(crate) prev_total: u64,
    pub(crate) ready: bool,
}

impl HostMeters {
    pub(crate) fn sample(&mut self) -> HostSnapshot {
        let (mem, mem_detail) = mem_stats();
        let (disk, disk_detail) = disk_root();
        HostSnapshot {
            cpu: self.cpu_percent(),
            gpu: gpu_busy(),
            mem,
            mem_detail,
            disk,
            disk_detail,
            power: crate::editor::power_label(),
        }
    }

    pub(crate) fn cpu_percent(&mut self) -> f32 {
        let Some((idle, total)) = cpu_times() else {
            return 0.0;
        };
        let percent = if self.ready && total > self.prev_total {
            let idle_d = idle.saturating_sub(self.prev_idle) as f32;
            let total_d = (total - self.prev_total) as f32;
            (1.0 - idle_d / total_d) * 100.0
        } else {
            0.0
        };
        self.prev_idle = idle;
        self.prev_total = total;
        self.ready = true;
        percent.clamp(0.0, 100.0)
    }
}

pub(crate) fn cpu_times() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let mut parts = text.lines().next()?.split_whitespace();
    if parts.next() != Some("cpu") {
        return None;
    }
    let nums: Vec<u64> = parts.filter_map(|part| part.parse().ok()).collect();
    if nums.len() < 4 {
        return None;
    }
    let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
    Some((idle, nums.iter().sum()))
}

pub(crate) fn mem_stats() -> (f32, String) {
    let text = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut total = 0u64;
    let mut available = 0u64;
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let kb = rest
            .split_whitespace()
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        match key {
            "MemTotal" => total = kb,
            "MemAvailable" => available = kb,
            _ => {}
        }
    }
    if total == 0 {
        return (0.0, "–".into());
    }
    let used = total.saturating_sub(available);
    let pct = used as f32 / total as f32 * 100.0;
    let gb = |kb: u64| kb as f32 / 1_048_576.0;
    (
        pct.clamp(0.0, 100.0),
        format!("{:.1} / {:.1} GB", gb(used), gb(total)),
    )
}

pub(crate) fn disk_root() -> (f32, String) {
    let Ok(output) = std::process::Command::new("df")
        .args(["-B1", "-P", "/"])
        .output()
    else {
        return (0.0, "–".into());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(line) = text.lines().nth(1) else {
        return (0.0, "–".into());
    };
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 3 {
        return (0.0, "–".into());
    }
    let total: u64 = cols[1].parse().unwrap_or(0);
    let used: u64 = cols[2].parse().unwrap_or(0);
    if total == 0 {
        return (0.0, "–".into());
    }
    let gb = |bytes: u64| bytes as f32 / 1_073_741_824.0;
    (
        (used as f32 / total as f32 * 100.0).clamp(0.0, 100.0),
        format!("{:.0} / {:.0} GB", gb(used), gb(total)),
    )
}

pub(crate) fn gpu_busy() -> f32 {
    for card in ["card1", "card0", "card2"] {
        let path = format!("/sys/class/drm/{card}/device/gpu_busy_percent");
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(value) = text.trim().parse::<f32>()
        {
            return value.clamp(0.0, 100.0);
        }
    }
    0.0
}

pub(crate) fn percent_label(value: f32) -> slint::SharedString {
    format!("{}%", value.round() as u32).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_sampling_never_blocks_ui_snapshot_reads() {
        let (started, sampling) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let monitor = HostMonitor::spawn(Arc::new(AtomicU64::new(500)), move || {
            started.send(()).unwrap();
            blocked.recv().unwrap();
            HostSnapshot {
                cpu: 42.0,
                ..Default::default()
            }
        });
        sampling.recv_timeout(Duration::from_secs(2)).unwrap();
        // The collector is deliberately blocked. A UI frame must still be able
        // to read the previous sample without waiting for it to finish.
        assert_eq!(monitor.snapshot().unwrap().cpu, 0.0);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !monitor
            .snapshot()
            .is_some_and(|snapshot| snapshot.cpu == 42.0)
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        // Dropping the monitor wakes the collector rather than waiting for the
        // dashboard refresh interval to expire.
        drop(monitor);
        assert!(sampling.recv_timeout(Duration::from_secs(2)).is_err());
    }
}
