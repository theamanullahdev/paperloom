//! The built-in widgets. Each one turns "now" into a pixmap and says how soon
//! it needs to be drawn again, so nothing runs more often than it must.

mod battery;
mod clock;
mod cpu;
mod date;
mod disk;
mod memory;
mod meter;
mod network;
mod uptime;

use std::time::Duration;

use tiny_skia::Pixmap;

use crate::render::Text;
use crate::sample::Sample;

pub struct Frame {
    pub pixmap: Pixmap,
    /// How long until what is shown changes.
    pub next_change: Duration,
}

/// What a widget may look at when it is drawn.
pub struct Context<'a> {
    pub sample: &'a Sample,
    /// On battery: meters update less often.
    pub slow: bool,
}

impl Context<'_> {
    /// How long a meter waits before looking again.
    pub fn meter_interval(&self) -> Duration {
        Duration::from_secs(if self.slow { 5 } else { 2 })
    }
}

/// Natural size of a widget in pixels at scale 1.0.
pub fn size_of(kind: &str) -> (u32, u32) {
    paperloom_common::widgets::natural_size(kind)
}

pub fn render(kind: &str, scale: f32, text: &mut Text, context: &Context) -> Option<Frame> {
    match kind {
        "clock" => Some(clock::render(scale, text)),
        "date" => Some(date::render(scale, text)),
        "cpu" => Some(cpu::render(scale, text, context)),
        "memory" => Some(memory::render(scale, text, context)),
        "disk" => Some(disk::render(scale, text, context)),
        "network" => Some(network::render(scale, text, context)),
        "battery" => Some(battery::render(scale, text, context)),
        "uptime" => Some(uptime::render(scale, text, context)),
        _ => None,
    }
}
