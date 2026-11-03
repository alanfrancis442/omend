// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs;
use std::path::Path;

/// `Some(true)` when a mains adapter is online, `Some(false)` on battery.
/// `None` when this machine has no mains supply to read.
pub fn mains_online() -> Option<bool> {
    mains_online_in(Path::new("/sys/class/power_supply"))
}

pub fn mains_online_in(root: &Path) -> Option<bool> {
    let entries = fs::read_dir(root).ok()?;
    let mut saw_mains = false;
    let mut online = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = fs::read_to_string(path.join("type")) else {
            continue;
        };
        if !kind.trim().eq_ignore_ascii_case("Mains") {
            continue;
        }
        saw_mains = true;
        let Ok(value) = fs::read_to_string(path.join("online")) else {
            continue;
        };
        if value.trim() == "1" {
            online = true;
        }
    }
    saw_mains.then_some(online)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_mains_wins_over_an_unplugged_adapter() {
        let root = std::env::temp_dir().join(format!("omen-power-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("BAT1")).unwrap();
        fs::write(root.join("BAT1/type"), "Battery\n").unwrap();
        fs::create_dir_all(root.join("AC0")).unwrap();
        fs::write(root.join("AC0/type"), "Mains\n").unwrap();
        fs::write(root.join("AC0/online"), "0\n").unwrap();
        assert_eq!(mains_online_in(&root), Some(false));
        fs::write(root.join("AC0/online"), "1\n").unwrap();
        assert_eq!(mains_online_in(&root), Some(true));
        let _ = fs::remove_dir_all(&root);
    }
}
