//! Stacks of widgets: a preset is a ready-made set of widgets with their looks
//! and places inside it; putting one on the desktop places every widget at
//! once, and from then on the whole stack moves and resizes as one. Presets
//! are built in or installed from the online library (one folder each with a
//! `stack.toml`). On the desktop a stack is nothing new: its widgets are
//! ordinary placed widgets that remember which stack they are in and where
//! (`Placed::stack`, `dx`, `dy`), so the widgets process needs no changes.

use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::widgets::{entry, Placed, Widgets, MAX_PLACED};

const BUILT_IN: &[&str] = &[
    include_str!("../stacks/saturday.toml"),
    include_str!("../stacks/neon-player.toml"),
    include_str!("../stacks/quiet-lines.toml"),
    include_str!("../stacks/morning.toml"),
    include_str!("../stacks/dashboard.toml"),
    include_str!("../stacks/monitor.toml"),
    include_str!("../stacks/pair.toml"),
    include_str!("../stacks/focus.toml"),
];

/// A ready-made set of widgets.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Preset {
    /// `stack/morning`, unique.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Width and height in pixels at natural size.
    pub size: [f32; 2],
    #[serde(default)]
    pub widget: Vec<PresetWidget>,
    /// Installed from the library (it can be removed); false for built-ins.
    #[serde(skip)]
    pub installed: bool,
    /// Its folder when installed.
    #[serde(skip)]
    pub folder: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PresetWidget {
    pub kind: String,
    /// The look (`clock/neon`); empty = the kind's default.
    #[serde(default)]
    pub skin: String,
    /// Top-left inside the stack, in pixels at natural size.
    pub dx: f32,
    pub dy: f32,
}

/// A stack on the desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stack {
    pub id: u32,
    pub name: String,
    /// The preset it came from.
    pub preset: String,
    /// Top-left as fractions of the screen.
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    /// Natural size in pixels (so it can be kept on the screen).
    pub size: [f32; 2],
}

impl Default for Stack {
    fn default() -> Self {
        Self { id: 1, name: String::new(), preset: String::new(), x: 0.06, y: 0.06, scale: 1.0, size: [300.0, 200.0] }
    }
}

impl Preset {
    fn parse(source: &str) -> Option<Self> {
        let preset: Preset = toml::from_str(source).ok()?;
        (!preset.id.is_empty() && !preset.widget.is_empty() && preset.widget.iter().all(|widget| entry(&widget.kind).is_some())).then_some(preset)
    }
}

/// Every preset: the built-in ones, then the ones installed from the library.
pub fn presets() -> Vec<Preset> {
    let mut all: Vec<Preset> = BUILT_IN.iter().filter_map(|source| Preset::parse(source)).collect();
    if let Ok(folders) = fs::read_dir(paths::stacks_dir()) {
        let mut folders: Vec<_> = folders.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
        folders.sort();
        for folder in folders {
            let Some(mut preset) = fs::read_to_string(folder.join("stack.toml")).ok().and_then(|source| Preset::parse(&source)) else { continue };
            preset.installed = true;
            preset.folder = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            // An installed one replaces a built-in with the same id.
            all.retain(|existing| existing.id != preset.id);
            all.push(preset);
        }
    }
    all
}

pub fn preset(id: &str) -> Option<Preset> {
    presets().into_iter().find(|preset| preset.id == id)
}

impl Widgets {
    pub fn stack(&self, id: u32) -> Option<&Stack> {
        self.stacks.iter().find(|stack| stack.id == id)
    }

    /// The widgets in a stack, in order.
    pub fn members(&self, stack: u32) -> impl Iterator<Item = &Placed> {
        self.placed.iter().filter(move |placed| placed.stack == stack && stack != 0)
    }

    /// Puts a preset on the desktop: all its widgets, as one stack. `None` when
    /// they wouldn't fit under the cap of widgets.
    pub fn add_stack(&mut self, preset: &Preset, screen: (u32, u32)) -> Option<u32> {
        if self.placed.len() + preset.widget.len() > MAX_PLACED {
            return None;
        }
        let id = self.stacks.iter().map(|stack| stack.id).max().unwrap_or(0) + 1;
        let step = (self.stacks.len() % 6) as f32 * 0.05;
        self.stacks.push(Stack { id, name: preset.name.clone(), preset: preset.id.clone(), x: 0.06 + step, y: 0.08 + step, scale: 1.0, size: preset.size });
        let first = self.placed.iter().map(|placed| placed.id).max().unwrap_or(0) + 1;
        for (next, member) in (first..).zip(&preset.widget) {
            self.placed.push(Placed { id: next, kind: member.kind.clone(), skin: member.skin.clone(), stack: id, dx: member.dx, dy: member.dy, ..Placed::default() });
        }
        self.arrange_stack(id, screen);
        Some(id)
    }

    /// Puts every widget of a stack where the stack's place and size say.
    pub fn arrange_stack(&mut self, id: u32, screen: (u32, u32)) {
        let Some(stack) = self.stack(id).cloned() else { return };
        let (w, h) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
        // The whole stack stays on the screen.
        let (max_x, max_y) = ((1.0 - stack.size[0] * stack.scale / w).max(0.0), (1.0 - stack.size[1] * stack.scale / h).max(0.0));
        let (x, y) = (stack.x.clamp(0.0, max_x), stack.y.clamp(0.0, max_y));
        if let Some(stack) = self.stacks.iter_mut().find(|stack| stack.id == id) {
            stack.x = x;
            stack.y = y;
        }
        for placed in self.placed.iter_mut().filter(|placed| placed.stack == id) {
            placed.x = x + placed.dx * stack.scale / w;
            placed.y = y + placed.dy * stack.scale / h;
            placed.scale = stack.scale;
        }
    }

    /// Moves a widget to (`x`, `y`). A widget in a stack takes its whole stack along.
    pub fn move_member(&mut self, id: u32, x: f32, y: f32, screen: (u32, u32)) {
        let Some((stack, was_x, was_y)) = self.placed.iter().find(|placed| placed.id == id).map(|placed| (placed.stack, placed.x, placed.y)) else { return };
        if stack == 0 {
            if let Some(placed) = self.placed.iter_mut().find(|placed| placed.id == id) {
                placed.x = x.clamp(0.0, 0.95);
                placed.y = y.clamp(0.0, 0.95);
            }
            return;
        }
        if let Some(target) = self.stacks.iter_mut().find(|target| target.id == stack) {
            target.x += x - was_x;
            target.y += y - was_y;
        }
        self.arrange_stack(stack, screen);
    }

    pub fn scale_stack(&mut self, id: u32, scale: f32, screen: (u32, u32)) {
        if let Some(stack) = self.stacks.iter_mut().find(|stack| stack.id == id) {
            stack.scale = scale.clamp(0.5, 3.0);
        }
        self.arrange_stack(id, screen);
    }

    /// Takes the whole stack off the desktop.
    pub fn remove_stack(&mut self, id: u32) {
        self.placed.retain(|placed| placed.stack != id);
        self.stacks.retain(|stack| stack.id != id);
    }

    /// Keeps the widgets where they are, as separate ones.
    pub fn ungroup(&mut self, id: u32) {
        for placed in self.placed.iter_mut().filter(|placed| placed.stack == id) {
            placed.stack = 0;
            placed.dx = 0.0;
            placed.dy = 0.0;
        }
        self.stacks.retain(|stack| stack.id != id);
    }

    /// Drops stacks nothing belongs to any more, and members of stacks that no longer exist.
    pub fn tidy_stacks(&mut self) {
        let known: Vec<u32> = self.stacks.iter().map(|stack| stack.id).collect();
        for placed in self.placed.iter_mut().filter(|placed| placed.stack != 0 && !known.contains(&placed.stack)) {
            placed.stack = 0;
        }
        let used: Vec<u32> = self.placed.iter().map(|placed| placed.stack).collect();
        self.stacks.retain(|stack| used.contains(&stack.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_presets_are_valid() {
        let all = presets();
        assert!(all.len() >= 8, "{} presets parsed", all.len());
        for preset in &all {
            for widget in &preset.widget {
                assert!(entry(&widget.kind).is_some(), "{}: {}", preset.id, widget.kind);
            }
        }
    }

    #[test]
    fn a_stack_moves_as_one() {
        let screen = (1920, 1080);
        let preset = preset("stack/pair").unwrap();
        let mut widgets = Widgets { enabled: true, placed: Vec::new(), ..Widgets::default() };
        let id = widgets.add_stack(&preset, screen).unwrap();
        assert_eq!(widgets.members(id).count(), 2);
        let first = widgets.placed[0].id;
        let (before_a, before_b) = (widgets.placed[0].x, widgets.placed[1].x);
        widgets.move_member(first, before_a + 0.2, widgets.placed[0].y, screen);
        assert!((widgets.placed[0].x - before_a - 0.2).abs() < 1e-4);
        assert!((widgets.placed[1].x - before_b - 0.2).abs() < 1e-4);
        widgets.scale_stack(id, 2.0, screen);
        assert!((widgets.placed[1].scale - 2.0).abs() < 1e-6);
        widgets.ungroup(id);
        assert!(widgets.stacks.is_empty() && widgets.placed.iter().all(|placed| placed.stack == 0));
    }

    #[test]
    fn a_stack_stays_on_the_screen() {
        let screen = (1920, 1080);
        let preset = preset("stack/dashboard").unwrap();
        let mut widgets = Widgets { enabled: true, placed: Vec::new(), ..Widgets::default() };
        let id = widgets.add_stack(&preset, screen).unwrap();
        widgets.move_member(widgets.placed[0].id, 5.0, 5.0, screen);
        let stack = widgets.stack(id).unwrap();
        assert!(stack.x + stack.size[0] / 1920.0 <= 1.0001 && stack.y + stack.size[1] / 1080.0 <= 1.0001);
    }

    #[test]
    fn the_cap_is_kept() {
        let screen = (1920, 1080);
        let preset = preset("stack/dashboard").unwrap();
        let mut widgets = Widgets { enabled: true, placed: Vec::new(), ..Widgets::default() };
        let mut added = 0;
        while widgets.add_stack(&preset, screen).is_some() {
            added += 1;
        }
        assert!(widgets.placed.len() <= MAX_PLACED && added >= 1);
    }
}
