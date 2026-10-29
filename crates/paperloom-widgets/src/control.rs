//! What the process watches besides drawing: whether `widgets.toml` changed,
//! whether the wallpaper engine says "paused" (the same fullscreen / lock /
//! battery rules, decided once, by the engine), and the battery. Looked at
//! every couple of seconds, never per frame.

use std::time::{Duration, Instant, SystemTime};

use paperloom_common::widgets::changed_at;
use paperloom_common::EngineState;

use crate::guard::Guard;
use crate::sample::Sample;

// A change made in the app shows on the desktop within a second.
const CHECK_EVERY: Duration = Duration::from_secs(1);

pub struct Control {
    seen: Option<SystemTime>,
    last_check: Instant,
    engine_paused: bool,
    hide_below: Option<u8>,
    guard: Guard,
    pub sample: Sample,
    /// Test mode: draw whatever the pause rules say, until this moment.
    test_until: Option<Instant>,
    reported: String,
}

impl Control {
    pub fn new() -> Self {
        let mut control = Self { seen: None, last_check: Instant::now(), engine_paused: false, hide_below: None, guard: Guard::new(), sample: Sample::new(), test_until: None, reported: String::new() };
        control.mark_seen();
        control
    }

    /// Test mode (`--test`): ignores the pause rules and ends after `seconds`.
    pub fn test_for(&mut self, seconds: u64) {
        self.test_until = Some(Instant::now() + Duration::from_secs(seconds));
    }

    pub fn testing(&self) -> bool {
        self.test_until.is_some()
    }

    pub fn test_over(&self) -> bool {
        self.test_until.is_some_and(|until| Instant::now() >= until)
    }

    /// Remembers the current `widgets.toml` as what is on screen.
    pub fn mark_seen(&mut self) {
        self.seen = changed_at();
        let settings = paperloom_common::load();
        self.hide_below = settings.pause_on_low_battery.then_some(settings.low_battery_percent);
    }

    /// True once `widgets.toml` differs from what is on screen. Also where the
    /// engine's pause state is refreshed.
    pub fn changed(&mut self) -> bool {
        self.guard.check();
        if self.test_over() {
            return true;
        }
        if self.last_check.elapsed() < CHECK_EVERY {
            return false;
        }
        self.last_check = Instant::now();
        self.engine_paused = EngineState::load().auto_paused;
        // Say why nothing shows (or that it does), where the developer page can read it.
        let why = self.pause_reason().unwrap_or_default();
        if why != self.reported {
            paperloom_common::log::info(&if why.is_empty() { "widgets: drawing".to_string() } else { format!("widgets: hidden — {why}") });
            self.reported.clone_from(&why);
            paperloom_common::widgets::Status::update(|status| status.paused = why);
        }
        changed_at() != self.seen
    }

    /// Nothing should be drawn: the wallpaper is paused automatically, or the
    /// battery is low and discharging.
    pub fn paused(&self) -> bool {
        self.pause_reason().is_some()
    }

    /// Why nothing is drawn, in words; `None` when it is drawing.
    pub fn pause_reason(&self) -> Option<String> {
        if self.testing() || paperloom_common::dev::DevSwitches::active().disable_auto_pause {
            return None;
        }
        if self.engine_paused {
            return Some("the wallpaper is paused automatically (a fullscreen or covering window, the lock screen, or battery rules)".into());
        }
        match (self.hide_below, self.sample.battery) {
            (Some(threshold), Some(battery)) if battery.discharging && battery.percent <= threshold => Some(format!("the battery is at {}% (hidden at {threshold}% or less)", battery.percent)),
            _ => None,
        }
    }

    /// On battery: meters update less often.
    pub fn slow(&self) -> bool {
        self.sample.battery.is_some_and(|battery| battery.discharging)
    }

    pub fn refresh_sample(&mut self) {
        self.sample.refresh();
    }
}
