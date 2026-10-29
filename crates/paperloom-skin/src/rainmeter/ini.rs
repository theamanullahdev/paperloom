//! Reading a Rainmeter `.ini`: sections in order, `@Include` files,
//! `[Variables]` with `#Name#` references, `MeterStyle`, and the small
//! arithmetic Rainmeter allows in parentheses (`X=(#Width#/2)`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One section: its name and its keys (lower-case) in file order.
#[derive(Debug, Clone, Default)]
pub struct Section {
    pub name: String,
    pub keys: Vec<(String, String)>,
}

impl Section {
    pub fn get(&self, key: &str) -> Option<&str> {
        // The last one wins, as in Rainmeter.
        self.keys.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Default)]
pub struct Ini {
    pub sections: Vec<Section>,
}

impl Ini {
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.name.eq_ignore_ascii_case(name))
    }

    /// Reads `path` and every file it includes, then fills in the variables.
    pub fn load(path: &Path, resources: &Path) -> Result<Self, String> {
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut ini = Ini::default();
        read_into(&mut ini, path, &dir, resources, "", 0)?;
        ini.resolve(&dir, resources);
        Ok(ini)
    }

    fn merge(&mut self, section: Section) {
        match self.sections.iter_mut().find(|existing| existing.name.eq_ignore_ascii_case(&section.name)) {
            Some(existing) => existing.keys.extend(section.keys),
            None => self.sections.push(section),
        }
    }

    /// Replaces `#Name#` everywhere and applies `MeterStyle`.
    fn resolve(&mut self, dir: &Path, resources: &Path) {
        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("@".into(), slash(resources));
        vars.insert("currentpath".into(), slash(dir));
        vars.insert("rootconfigpath".into(), slash(resources.parent().unwrap_or(dir)));
        vars.insert("skinspath".into(), slash(resources.parent().and_then(Path::parent).unwrap_or(dir)));
        vars.insert("crlf".into(), "\n".into());
        if let Some(section) = self.section("Variables") {
            for (key, value) in &section.keys {
                vars.insert(key.clone(), value.clone());
            }
        }
        // Variables may refer to each other; a few passes settle it.
        for _ in 0..4 {
            let snapshot = vars.clone();
            for value in vars.values_mut() {
                *value = substitute(value, &snapshot);
            }
        }
        for section in &mut self.sections {
            for (_, value) in &mut section.keys {
                *value = substitute(value, &vars);
            }
        }
        // MeterStyle: the style's keys come first, so the meter's own win.
        let styles: Vec<Section> = self.sections.clone();
        for section in &mut self.sections {
            let Some(names) = section.get("meterstyle").map(str::to_string) else { continue };
            let mut inherited = Vec::new();
            for name in names.split('|').map(str::trim) {
                if let Some(style) = styles.iter().find(|style| style.name.eq_ignore_ascii_case(name)) {
                    inherited.extend(style.keys.iter().cloned());
                }
            }
            inherited.append(&mut section.keys);
            section.keys = inherited;
        }
    }
}

/// Reads `path` into `ini`; keys before its first `[Section]` belong to `section`
/// (an `@Include` inside `[Variables]` adds variables).
fn read_into(ini: &mut Ini, path: &Path, dir: &Path, resources: &Path, section: &str, depth: u32) -> Result<(), String> {
    if depth > 4 {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|err| format!("couldn't read {}: {err}", path.display()))?;
    let source = decode(&bytes);
    let mut current = Section { name: section.to_string(), keys: Vec::new() };
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            ini.merge(std::mem::take(&mut current));
            current.name = name.trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let value = strip_quotes(value.trim()).to_string();
        if key.starts_with("@include") {
            let mut builtins = HashMap::new();
            builtins.insert("@".to_string(), slash(resources));
            builtins.insert("currentpath".to_string(), slash(dir));
            let target = PathBuf::from(substitute(&value, &builtins).replace('\\', "/"));
            let target = if target.is_absolute() { target } else { dir.join(target) };
            // Includes never leave the skin's own folders.
            if within(&target, resources.parent().unwrap_or(dir)) {
                let name = current.name.clone();
                ini.merge(std::mem::replace(&mut current, Section { name: name.clone(), keys: Vec::new() }));
                let _ = read_into(ini, &target, dir, resources, &name, depth + 1);
            }
            continue;
        }
        current.keys.push((key, value));
    }
    ini.merge(current);
    Ok(())
}

/// Rainmeter files are often UTF-16 with a byte-order mark.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)).into_owned()
}

fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn slash(path: &Path) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if !text.ends_with('/') {
        text.push('/');
    }
    text
}

fn within(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

/// `#Name#` → its value (names are case-insensitive); unknown ones stay as written.
fn substitute(text: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('#') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('#') {
            Some(end) if !after[..end].contains(char::is_whitespace) && end > 0 => match vars.get(&after[..end].to_ascii_lowercase()) {
                Some(value) => {
                    out.push_str(value);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('#');
                    rest = after;
                }
            },
            _ => {
                out.push('#');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A number, or a formula in parentheses: `12`, `-4.5`, `(200/2+#pad#)`.
pub fn number(text: &str) -> Option<f32> {
    let text = text.trim();
    if let Ok(value) = text.parse::<f32>() {
        return Some(value);
    }
    let mut parser = Expr { chars: text.chars().filter(|c| !c.is_whitespace()).collect(), at: 0 };
    let value = parser.sum()?;
    (parser.at == parser.chars.len()).then_some(value as f32)
}

struct Expr {
    chars: Vec<char>,
    at: usize,
}

impl Expr {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.at += 1;
            let right = self.product()?;
            value = if op == '+' { value + right } else { value - right };
        }
        Some(value)
    }

    fn product(&mut self) -> Option<f64> {
        let mut value = self.unary()?;
        while let Some(op @ ('*' | '/' | '%')) = self.peek() {
            self.at += 1;
            let right = self.unary()?;
            value = match op {
                '*' => value * right,
                '/' if right != 0.0 => value / right,
                '%' if right != 0.0 => value % right,
                _ => 0.0,
            };
        }
        Some(value)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            '-' => {
                self.at += 1;
                Some(-self.unary()?)
            }
            '+' => {
                self.at += 1;
                self.unary()
            }
            '(' => {
                self.at += 1;
                let value = self.sum()?;
                (self.peek()? == ')').then(|| self.at += 1)?;
                Some(value)
            }
            _ => {
                let start = self.at;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.at += 1;
                }
                self.chars[start..self.at].iter().collect::<String>().parse().ok()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas() {
        assert_eq!(number("12"), Some(12.0));
        assert_eq!(number("(200/2+5)"), Some(105.0));
        assert_eq!(number("(-(3*2))"), Some(-6.0));
        assert_eq!(number("abc"), None);
    }

    #[test]
    fn variables() {
        let mut vars = HashMap::new();
        vars.insert("size".to_string(), "20".to_string());
        assert_eq!(substitute("(#Size#*2) #missing# #a b#", &vars), "(20*2) #missing# #a b#");
    }
}
