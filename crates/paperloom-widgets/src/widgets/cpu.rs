use super::{meter, Context, Frame};
use crate::render::Text;

pub fn render(scale: f32, text: &mut Text, context: &Context) -> Frame {
    let cpu = context.sample.cpu;
    let pixmap = meter::draw("cpu", scale, text, "Processor", &format!("{cpu:.0}%"), "of all cores", Some(cpu / 100.0));
    Frame { pixmap, next_change: context.meter_interval() }
}
