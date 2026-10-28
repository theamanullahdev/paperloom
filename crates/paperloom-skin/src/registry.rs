//! The skins that ship with Paperloom, parsed once. (Installed and downloaded
//! skins will join them through the same format.)

use std::sync::OnceLock;

use crate::element::Skin;

const BUILT_IN: &[&str] = &[
    include_str!("../skins/clock-card.toml"),
    include_str!("../skins/clock-minimal.toml"),
    include_str!("../skins/clock-analog.toml"),
    include_str!("../skins/clock-neon.toml"),
    include_str!("../skins/clock-ring.toml"),
    include_str!("../skins/date-card.toml"),
    include_str!("../skins/date-minimal.toml"),
    include_str!("../skins/date-badge.toml"),
    include_str!("../skins/meter-card.toml"),
    include_str!("../skins/meter-compact.toml"),
    include_str!("../skins/meter-ring.toml"),
    include_str!("../skins/meter-gauge.toml"),
    include_str!("../skins/meter-neon.toml"),
];

pub fn all() -> &'static [Skin] {
    static SKINS: OnceLock<Vec<Skin>> = OnceLock::new();
    SKINS.get_or_init(|| {
        BUILT_IN
            .iter()
            .filter_map(|source| match Skin::parse(source) {
                Ok(skin) => Some(skin),
                Err(err) => {
                    eprintln!("paperloom-skin: a built-in skin didn't parse: {err}");
                    None
                }
            })
            .collect()
    })
}

pub fn get(id: &str) -> Option<&'static Skin> {
    all().iter().find(|skin| skin.id == id)
}

/// Every skin that can show `kind`, in the order they ship.
pub fn for_kind(kind: &str) -> impl Iterator<Item = &'static Skin> + '_ {
    all().iter().filter(move |skin| skin.kinds.iter().any(|candidate| candidate == kind))
}

/// The look a new widget of `kind` starts with: its `…/card`.
pub fn default_for(kind: &str) -> Option<&'static Skin> {
    for_kind(kind).find(|skin| skin.id.ends_with("/card")).or_else(|| for_kind(kind).next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{preview, render, Text};

    #[test]
    fn every_built_in_skin_parses_and_draws_for_each_of_its_kinds() {
        assert_eq!(all().len(), BUILT_IN.len(), "a built-in skin failed to parse");
        let mut text = Text::new();
        for skin in all() {
            for kind in &skin.kinds {
                let pixmap = render(skin, &preview(kind), 1.0, &mut text);
                assert!(pixmap.pixels().iter().any(|pixel| pixel.alpha() > 0), "{} draws nothing for {kind}", skin.id);
            }
        }
    }

    #[test]
    fn every_kind_has_a_default_look() {
        for kind in ["clock", "date", "cpu", "memory", "disk", "network", "battery", "uptime"] {
            assert!(default_for(kind).is_some(), "{kind} has no look");
        }
    }
}
