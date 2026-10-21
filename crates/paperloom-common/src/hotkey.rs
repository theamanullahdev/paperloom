//! Global shortcut text such as "Ctrl+Alt+Right" → modifiers and a key, with
//! the numbers each platform's grab call wants.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    key: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    /// A letter or digit, upper-case.
    Char(char),
    /// F1..=F12.
    Function(u8),
    Left,
    Right,
    Up,
    Down,
    Space,
    Enter,
    Home,
    End,
    PageUp,
    PageDown,
}

impl Hotkey {
    /// `None` for empty or unrecognised text, or for a bare key with no
    /// modifier (which would steal normal typing everywhere).
    pub fn parse(text: &str) -> Option<Self> {
        let mut hotkey = Hotkey { ctrl: false, alt: false, shift: false, super_key: false, key: Key::Space };
        let mut key = None;
        for part in text.split('+').map(|part| part.trim().to_lowercase()).filter(|part| !part.is_empty()) {
            match part.as_str() {
                "ctrl" | "control" => hotkey.ctrl = true,
                "alt" | "option" => hotkey.alt = true,
                "shift" => hotkey.shift = true,
                "super" | "win" | "windows" | "meta" | "cmd" => hotkey.super_key = true,
                "left" => key = Some(Key::Left),
                "right" => key = Some(Key::Right),
                "up" => key = Some(Key::Up),
                "down" => key = Some(Key::Down),
                "space" => key = Some(Key::Space),
                "enter" | "return" => key = Some(Key::Enter),
                "home" => key = Some(Key::Home),
                "end" => key = Some(Key::End),
                "pageup" => key = Some(Key::PageUp),
                "pagedown" => key = Some(Key::PageDown),
                other => {
                    if let Some(number) = other.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=12).contains(n)) {
                        key = Some(Key::Function(number));
                    } else {
                        let mut chars = other.chars();
                        match (chars.next(), chars.next()) {
                            (Some(c), None) if c.is_ascii_alphanumeric() => key = Some(Key::Char(c.to_ascii_uppercase())),
                            _ => return None,
                        }
                    }
                }
            }
        }
        hotkey.key = key?;
        (hotkey.ctrl || hotkey.alt || hotkey.shift || hotkey.super_key).then_some(hotkey)
    }

    /// X11 keysym of the key.
    pub fn x11_keysym(&self) -> u32 {
        match self.key {
            Key::Char(c) => c.to_ascii_lowercase() as u32,
            Key::Function(n) => 0xffbe + u32::from(n) - 1,
            Key::Left => 0xff51,
            Key::Up => 0xff52,
            Key::Right => 0xff53,
            Key::Down => 0xff54,
            Key::Space => 0x20,
            Key::Enter => 0xff0d,
            Key::Home => 0xff50,
            Key::End => 0xff57,
            Key::PageUp => 0xff55,
            Key::PageDown => 0xff56,
        }
    }

    /// X11 modifier mask (Shift=1, Control=4, Mod1/Alt=8, Mod4/Super=64).
    pub fn x11_modifiers(&self) -> u16 {
        u16::from(self.shift) | (u16::from(self.ctrl) << 2) | (u16::from(self.alt) << 3) | (u16::from(self.super_key) << 6)
    }

    /// Windows virtual-key code.
    pub fn windows_vk(&self) -> u32 {
        match self.key {
            Key::Char(c) => c as u32,
            Key::Function(n) => 0x70 + u32::from(n) - 1,
            Key::Left => 0x25,
            Key::Up => 0x26,
            Key::Right => 0x27,
            Key::Down => 0x28,
            Key::Space => 0x20,
            Key::Enter => 0x0D,
            Key::Home => 0x24,
            Key::End => 0x23,
            Key::PageUp => 0x21,
            Key::PageDown => 0x22,
        }
    }

    /// Windows `RegisterHotKey` modifier flags (Alt=1, Ctrl=2, Shift=4, Win=8).
    pub fn windows_modifiers(&self) -> u32 {
        u32::from(self.alt) | (u32::from(self.ctrl) << 1) | (u32::from(self.shift) << 2) | (u32::from(self.super_key) << 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_shortcuts() {
        let next = Hotkey::parse("Ctrl+Alt+Right").unwrap();
        assert!(next.ctrl && next.alt && !next.shift);
        assert_eq!(next.x11_keysym(), 0xff53);
        assert_eq!(next.x11_modifiers(), 4 | 8);
        assert_eq!(next.windows_vk(), 0x27);
        assert_eq!(next.windows_modifiers(), 1 | 2);
        assert_eq!(Hotkey::parse("super + p").unwrap().x11_keysym(), 'p' as u32);
        assert_eq!(Hotkey::parse("Ctrl+F5").unwrap().windows_vk(), 0x74);
    }

    #[test]
    fn rejects_unsafe_or_unknown() {
        assert!(Hotkey::parse("").is_none());
        assert!(Hotkey::parse("P").is_none()); // no modifier
        assert!(Hotkey::parse("Ctrl+Banana").is_none());
        assert!(Hotkey::parse("Ctrl+Alt").is_none()); // no key
    }
}
