use super::{meter, Context, Frame};
use crate::render::Text;

pub fn render(scale: f32, text: &mut Text, context: &Context) -> Frame {
    let seconds = sysinfo::System::uptime();
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3_600 % 24, seconds / 60 % 60);
    let value = if days > 0 { format!("{days}d {hours}h") } else if hours > 0 { format!("{hours}h {minutes}m") } else { format!("{minutes}m") };
    let pixmap = meter::draw("uptime", scale, text, "Uptime", &value, "since this computer started", None);
    Frame { pixmap, next_change: context.meter_interval() * 12 }
}
