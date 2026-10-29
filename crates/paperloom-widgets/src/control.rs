//! What the process watches besides drawing: whether `widgets.toml` changed,
//! whether the wallpaper engine says "paused" (the same fullscreen / lock /
//! battery rules, decided once, by the engine), and the battery. Looked at
//! every couple of seconds, never per frame.

use std::time::{Duration, Instant, SystemTime};

use paperloom_common::widgets::changed_at;
use paperloom_common::EngineState;

use crate::guard::Guard;
use crate::sample::Sample;

const CHECK_EVERY: Duration = Duration::from_secs(2);

pub struct Control {
    seen: Option<SystemTime>,
    last_check: Instant,
    engine_paused: bool,
    hide_below: Option<u8>,
    guard: Guard,
    pub sample: Sample,
}

impl Control {
    pub fn new() -> Self {
        let mut control = Self { seen: None, last_check: Instant::now(), engine_paused: false, hide_below: None, guard: Guard::new(), sample: Sample::new() };
        control.mark_seen();
        control
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
        if self.last_check.elapsed() < CHECK_EVERY {
            return false;
        }
        self.last_check = Instant::now();
        self.engine_paused = EngineState::load().auto_paused;
        changed_at() != self.seen
    }

    /// Nothing should be drawn: the wallpaper is paused automatically, or the
    /// battery is low and discharging.
    pub fn paused(&self) -> bool {
        let low = match (self.hide_below, self.sample.battery) {
            (Some(threshold), Some(battery)) => battery.discharging && battery.percent <= threshold,
            _ => false,
        };
        self.engine_paused || low
    }

    /// On battery: meters update less often.
    pub fn slow(&self) -> bool {
        self.sample.battery.is_some_and(|battery| battery.discharging)
    }

    pub fn refresh_sample(&mut self) {
        self.sample.refresh();
    }
}
