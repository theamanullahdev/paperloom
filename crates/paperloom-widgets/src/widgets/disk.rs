use super::{meter, Context, Frame};
use crate::render::Text;

pub fn render(scale: f32, text: &mut Text, context: &Context) -> Frame {
    let sample = context.sample;
    let fraction = if sample.disk_total > 0 { sample.disk_used as f32 / sample.disk_total as f32 } else { 0.0 };
    let detail = format!("{} free of {}", meter::size(sample.disk_total.saturating_sub(sample.disk_used)), meter::size(sample.disk_total));
    let pixmap = meter::draw("disk", scale, text, "Disk", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction));
    // Free space changes slowly.
    Frame { pixmap, next_change: context.meter_interval() * 6 }
}
