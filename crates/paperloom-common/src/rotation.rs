//! Wallpaper rotation: change the wallpaper by itself, either on an
//! interval from one collection, or by a weekly schedule ("evenings: Rainy
//! collection"). Stored in `rotation.toml`, written by the app and read by
//! the engine, which is the process that is always running.

use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rotation {
    /// Rotate through `collection` all the time (when no schedule rule applies).
    pub enabled: bool,
    pub collection: Option<u64>,
    /// Minutes between changes; 0 keeps the current wallpaper until the
    /// collection changes.
    pub interval_minutes: u32,
    pub shuffle: bool,
    pub rules: Vec<Rule>,
}

/// "From `start` to `end` on these days, use this collection."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub enabled: bool,
    pub collection: u64,
    /// Minutes after midnight, local time.
    pub start: u16,
    pub end: u16,
    /// Bit 0 = Monday … bit 6 = Sunday.
    pub days: u8,
}

impl Default for Rule {
    fn default() -> Self {
        Self { enabled: true, collection: 0, start: 18 * 60, end: 23 * 60, days: 0b111_1111 }
    }
}

/// The local weekday (Monday = 0) and minutes since midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub weekday: u8,
    pub minutes: u16,
}

impl Rule {
    /// A rule that crosses midnight (e.g. 22:00–06:00) belongs to the day
    /// it started on.
    pub fn applies(&self, now: LocalTime) -> bool {
        if !self.enabled {
            return false;
        }
        let on = |day: u8| self.days & (1 << (day % 7)) != 0;
        if self.start == self.end {
            on(now.weekday)
        } else if self.start < self.end {
            on(now.weekday) && now.minutes >= self.start && now.minutes < self.end
        } else if now.minutes >= self.start {
            on(now.weekday)
        } else if now.minutes < self.end {
            on((now.weekday + 6) % 7)
        } else {
            false
        }
    }
}

impl Rotation {
    pub fn load() -> Self {
        fs::read_to_string(paths::rotation_file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::rotation_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self).expect("Rotation always serializes"))
    }

    /// The collection that should be playing now: the first matching
    /// schedule rule, else the always-on rotation, else none.
    pub fn wanted(&self, now: LocalTime) -> Option<u64> {
        self.rules.iter().find(|rule| rule.applies(now)).map(|rule| rule.collection).or(self.enabled.then_some(self.collection).flatten())
    }
}

/// Local time from the operating system (no time-zone database needed).
#[cfg(unix)]
pub fn local_now() -> LocalTime {
    // SAFETY: `localtime_r` fills the struct we own from a time value we own.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut parts: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut parts);
        LocalTime { weekday: ((parts.tm_wday + 6) % 7) as u8, minutes: (parts.tm_hour * 60 + parts.tm_min) as u16 }
    }
}

#[cfg(windows)]
pub fn local_now() -> LocalTime {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: fills the struct we own.
    let time = unsafe {
        let mut time = std::mem::zeroed();
        GetLocalTime(&mut time);
        time
    };
    LocalTime { weekday: ((time.wDayOfWeek + 6) % 7) as u8, minutes: time.wHour * 60 + time.wMinute }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(weekday: u8, hour: u16, minute: u16) -> LocalTime {
        LocalTime { weekday, minutes: hour * 60 + minute }
    }

    #[test]
    fn day_and_evening_rules() {
        let evening = Rule { collection: 2, start: 18 * 60, end: 23 * 60, days: 0b001_1111, ..Rule::default() }; // Mon–Fri
        assert!(evening.applies(at(0, 19, 0)));
        assert!(!evening.applies(at(0, 17, 59)));
        assert!(!evening.applies(at(0, 23, 0)));
        assert!(!evening.applies(at(5, 19, 0))); // Saturday
    }

    #[test]
    fn rules_across_midnight_belong_to_the_day_they_started() {
        let night = Rule { collection: 3, start: 22 * 60, end: 6 * 60, days: 0b000_0001, ..Rule::default() }; // Mondays
        assert!(night.applies(at(0, 23, 0)));
        assert!(night.applies(at(1, 2, 0))); // Tuesday 02:00 is still Monday night
        assert!(!night.applies(at(0, 2, 0))); // Monday 02:00 is Sunday night
        assert!(!night.applies(at(1, 23, 0)));
    }

    #[test]
    fn schedule_beats_the_always_on_rotation() {
        let rotation = Rotation { enabled: true, collection: Some(1), rules: vec![Rule { collection: 9, start: 0, end: 12 * 60, ..Rule::default() }], ..Rotation::default() };
        assert_eq!(rotation.wanted(at(2, 8, 0)), Some(9));
        assert_eq!(rotation.wanted(at(2, 15, 0)), Some(1));
        assert_eq!(Rotation::default().wanted(at(2, 15, 0)), None);
    }
}
