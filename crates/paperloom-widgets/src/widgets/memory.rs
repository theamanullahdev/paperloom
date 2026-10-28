use super::{meter, Context, Frame};
use crate::render::Text;

pub fn render(scale: f32, text: &mut Text, context: &Context) -> Frame {
    let sample = context.sample;
    let fraction = if sample.memory_total > 0 { sample.memory_used as f32 / sample.memory_total as f32 } else { 0.0 };
    let detail = format!("{} of {}", meter::size(sample.memory_used), meter::size(sample.memory_total));
    let pixmap = meter::draw("memory", scale, text, "Memory", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction));
    Frame { pixmap, next_change: context.meter_interval() }
}
