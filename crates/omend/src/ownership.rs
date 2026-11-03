// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs::{File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use anyhow::Context;

/// The production path is directly under root-owned /run, outside user control.
/// Dropping the file releases the kernel lock, including after process death.
pub fn acquire(path: &Path) -> anyhow::Result<File> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        anyhow::ensure!(
            metadata.is_file(),
            "{} must be a regular file",
            path.display()
        );
        anyhow::ensure!(
            metadata.uid() == effective_uid(),
            "{} has an unexpected owner",
            path.display()
        );
        anyhow::ensure!(
            metadata.mode() & 0o077 == 0,
            "{} must not grant group/other access",
            path.display()
        );
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("open daemon ownership lock {}", path.display()))?;
    file.try_lock()
        .with_context(|| format!("another daemon holds the hardware lock {}", path.display()))?;
    Ok(file)
}

fn effective_uid() -> u32 {
    // The production caller has already verified root. Metadata on /proc/self
    // is also usable in unprivileged, isolated tests, without a libc dependency.
    std::fs::metadata("/proc/self")
        .expect("Linux procfs is required")
        .uid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_is_exclusive_and_released_without_unlinking() {
        let path = std::env::temp_dir().join(format!("omend-lock-{}", std::process::id()));
        let first = acquire(&path).unwrap();
        assert!(acquire(&path).is_err());
        drop(first);
        let second = acquire(&path).unwrap();
        drop(second);
        std::fs::remove_file(path).unwrap();
    }
}
