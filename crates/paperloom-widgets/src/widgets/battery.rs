use super::{meter, Context, Frame};
use crate::render::Text;

pub fn render(scale: f32, text: &mut Text, context: &Context) -> Frame {
    let pixmap = match context.sample.battery {
        Some(battery) => {
            let detail = if battery.discharging { "On battery" } else { "Plugged in" };
            meter::draw("battery", scale, text, "Battery", &format!("{}%", battery.percent), detail, Some(f32::from(battery.percent) / 100.0))
        }
        None => meter::draw("battery", scale, text, "Battery", "None", "This computer has no battery", None),
    };
    // The sampler reads the battery every 30 s.
    Frame { pixmap, next_change: context.meter_interval() * 6 }
}
