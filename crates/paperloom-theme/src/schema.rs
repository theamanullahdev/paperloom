//! The theme format in words, for people editing the raw file and for AI
//! agents writing a theme (served by the MCP server).

pub const SCHEMA: &str = r##"# A Paperloom theme: theme.toml

# A theme is a folder holding this file, plus optional fonts/ and images/.
# Everything is optional: whatever is left out keeps Paperloom's default.
# Colours are "#RRGGBB" or "#RRGGBBAA" (AA = opacity).

id = "my-theme"            # letters, digits, - and _
name = "My theme"
author = "You"
description = "One line about it."

# Colours for each mode. Give one mode only and it is used for both.
[colors.dark]
background = "#1C1917"       # behind everything
surface = "#262220"          # navigation, cards and pop-ups
muted = "#2E2824"            # quiet buttons and sunken areas
border = "#FFFFFF14"         # outlines and dividing lines
foreground = "#F5EFE9"       # text
muted_foreground = "#A8998E" # secondary text
primary = "#FB923C"          # the accent: main buttons, the current page
primary_hover = "#FDBA74"    # the accent while pointed at
success = "#34D399"          # good news (now playing, on)
destructive = "#F87171"      # remove and delete
overlay = "#00000080"        # the dimming behind a pop-up
shadow = "#00000066"         # shadows

[colors.light]
# the same names

[fonts]
body = "IBM Plex Sans"         # family for ordinary text (applies after a restart)
display = "Bricolage Grotesque" # family for page titles
files = ["fonts/MyFont.ttf"]   # font files the theme brings (TTF/OTF)

[shape]
control_radius = 4   # buttons, inputs, chips (0-24)
card_radius = 6      # cards and panels (0-32)
popup_radius = 8     # pop-ups (0-40)
border_width = 1     # outlines (0-4)
shadow = 1           # 0 = none, 1 = normal, up to 3
scale = 1            # size of everything: 0.7-1.6

[layout]
nav = "left"            # left | right | top | bottom
nav_style = "full"      # full (icons and labels) | icons | hidden (menu button only)
nav_width = 200         # side navigation with labels, 150-360
search = true           # the search box in the top bar
title = true            # the page title in the top bar
content_max_width = 0   # widest the page may get; 0 = the whole window

[backdrop]              # what is behind the pages
gradient = ["#1C1917", "#3B2A20"]  # two or more colours; leave out for none
angle = 90              # 0 = left to right, 90 = top to bottom
image = "images/back.png"          # a PNG in the theme's folder
opacity = 1             # 0-1
fit = "cover"           # cover | contain | stretch
# Tip: give surface/background an AA below FF so the backdrop shows through.

[[layer]]               # a picture in the window; repeat [[layer]] for more
image = "images/snake.png"
# frames = ["images/s1.png", "images/s2.png"]  # instead of image: an animation
fps = 8                 # 1-30
anchor = "bottom-right" # top-left top top-right left center right bottom-left bottom bottom-right
width = 0               # pixels; 0 = the picture's own size
height = 0
margin = 0              # distance from the anchored edge
opacity = 1
front = false           # true = in front of the pages (never takes clicks)
pages = []              # only on these: library online widgets settings; empty = everywhere

# Restyle one part of the window. Parts: nav, top_bar, button_primary,
# button_secondary, button_danger, button_ghost, modal, screen, popup.
[part.nav]
background = "#141210"
foreground = "#F5EFE9"
border = "#FFFFFF22"
border_width = 1
radius = 0
"##;

#[cfg(test)]
mod tests {
    #[test]
    fn the_schema_example_is_itself_a_valid_theme() {
        // Font and picture files it names don't exist here, so check without a folder.
        let report = crate::check(super::SCHEMA, None);
        assert!(report.usable(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }
}
