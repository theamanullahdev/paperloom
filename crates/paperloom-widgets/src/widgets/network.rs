use super::{meter, Context, Frame};
use crate::render::Text;

pub fn render(scale: f32, text: &mut Text, context: &Context) -> Frame {
    let sample = context.sample;
    let detail = format!("Upload {}", meter::speed(sample.upload));
    let pixmap = meter::draw("network", scale, text, "Download", &meter::speed(sample.download), &detail, None);
    Frame { pixmap, next_change: context.meter_interval() }
}
