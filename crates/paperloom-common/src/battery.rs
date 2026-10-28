//! Battery charge and whether the machine is running on it. Read by the
//! widgets (slower updates on battery, hidden when low); the wallpaper
//! engine keeps its own copies next to its pause checks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    /// Running on the battery (not plugged in).
    pub discharging: bool,
}

/// `None` on a machine without a battery.
pub fn read() -> Option<Battery> {
    imp::read()
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::Battery;

    pub fn read() -> Option<Battery> {
        for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
            let path = entry.path();
            if std::fs::read_to_string(path.join("type")).ok()?.trim() != "Battery" {
                continue;
            }
            let percent = std::fs::read_to_string(path.join("capacity")).ok()?.trim().parse::<u8>().ok()?;
            let status = std::fs::read_to_string(path.join("status")).unwrap_or_default();
            return Some(Battery { percent: percent.min(100), discharging: status.trim() == "Discharging" });
        }
        None
    }
}

#[cfg(windows)]
mod imp {
    use super::Battery;
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    pub fn read() -> Option<Battery> {
        // SAFETY: fills one plain struct we own.
        let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { GetSystemPowerStatus(&mut status) } == 0 || status.BatteryLifePercent > 100 || status.BatteryFlag & 128 != 0 {
            return None; // call failed, charge unknown, or "no system battery"
        }
        Some(Battery { percent: status.BatteryLifePercent, discharging: status.ACLineStatus == 0 })
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Battery;

    pub fn read() -> Option<Battery> {
        let output = std::process::Command::new("pmset").args(["-g", "batt"]).output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let percent = text.split('%').next()?.rsplit(|c: char| !c.is_ascii_digit()).next()?.parse::<u8>().ok()?;
        Some(Battery { percent: percent.min(100), discharging: text.contains("discharging") })
    }
}
