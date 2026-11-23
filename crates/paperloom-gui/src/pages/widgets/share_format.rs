//! A widget, a look or a stack of widgets as one piece of text: a code to
//! paste in a chat (`paperloom-widget:1:…`) or the same text in a
//! `.paperloomwidget` file. Everything in it is plain data (no scripts);
//! whatever arrives is checked the way the online library's items are:
//! allowed file kinds only, safe paths, a size limit, a look that parses, and
//! never a look that would replace one that ships with Paperloom.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use paperloom_common::widgets::{self, Placed, MAX_PLACED};
use paperloom_fetch::share::{base64url_decode, base64url_encode};
use serde::{Deserialize, Serialize};

pub const PREFIX: &str = "paperloom-widget:1:";
/// A code longer than this is too much for a chat message: offer the file.
pub const CODE_LIMIT: usize = 24_000;
const MAX_BYTES: u64 = 12 * 1024 * 1024;
/// The widget settings a share may carry.
const OPTIONS: &[&str] = &["timezone"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shared {
    pub v: u32,
    pub name: String,
    /// The widgets: one for a widget, several for a stack, none for a look.
    #[serde(default)]
    pub widgets: Vec<Member>,
    /// Present for a stack: its size in pixels at natural size.
    #[serde(default)]
    pub stack: Option<[f32; 2]>,
    /// The looks that come with it (those not built into Paperloom).
    #[serde(default)]
    pub looks: Vec<LookFiles>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub kind: String,
    /// The look (`clock/neon`); empty = the kind's default.
    #[serde(default)]
    pub look: String,
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// Place inside a stack, in pixels at natural size.
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookFiles {
    pub id: String,
    /// Path inside the look's folder → the file, base64url.
    pub files: BTreeMap<String, String>,
}

/// What a share holds, for the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Widget,
    Stack,
    Look,
}

impl Shared {
    pub fn what(&self) -> What {
        if self.stack.is_some() {
            What::Stack
        } else if self.widgets.is_empty() {
            What::Look
        } else {
            What::Widget
        }
    }

    /// The text to paste or save.
    pub fn encode(&self) -> Result<String, String> {
        let json = serde_json::to_vec(self).map_err(|err| err.to_string())?;
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&json).map_err(|err| err.to_string())?;
        Ok(format!("{PREFIX}{}", base64url_encode(&encoder.finish().map_err(|err| err.to_string())?)))
    }

    /// Reads and checks a pasted code or a file's text.
    pub fn decode(text: &str) -> Result<Shared, String> {
        let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let body = cleaned.strip_prefix(PREFIX).ok_or_else(|| {
            if cleaned.starts_with("paperloom-widget:") {
                "This was made by a newer Paperloom. Update from Settings → Updates.".to_string()
            } else {
                "This isn't a Paperloom widget code.".to_string()
            }
        })?;
        let damaged = || "The code is damaged or cut short.".to_string();
        let packed = base64url_decode(body).ok_or_else(damaged)?;
        let mut json = Vec::new();
        DeflateDecoder::new(packed.as_slice()).take(MAX_BYTES * 2).read_to_end(&mut json).map_err(|_| damaged())?;
        let mut shared: Shared = serde_json::from_slice(&json).map_err(|_| damaged())?;
        shared.check()?;
        Ok(shared)
    }

    /// Cleans and checks everything a stranger's share can say.
    fn check(&mut self) -> Result<(), String> {
        if self.v != 1 {
            return Err("This was made by a newer Paperloom. Update from Settings → Updates.".into());
        }
        self.name = self.name.trim().chars().take(80).collect();
        if self.widgets.len() > MAX_PLACED || self.looks.len() > 24 {
            return Err("This holds more than Paperloom allows.".into());
        }
        if self.stack.is_some() && self.widgets.is_empty() {
            return Err("This stack has no widgets in it.".into());
        }
        if let Some(size) = &mut self.stack {
            size[0] = size[0].clamp(40.0, 4000.0);
            size[1] = size[1].clamp(40.0, 4000.0);
        }
        for member in &mut self.widgets {
            if widgets::entry(&member.kind).is_none() {
                return Err(format!("This uses a widget (“{}”) this version of Paperloom doesn't have.", member.kind.chars().take(30).collect::<String>()));
            }
            if !member.look.is_empty() && !valid_id(&member.look) {
                return Err("This names a look in a way that isn't allowed.".into());
            }
            member.scale = if member.scale.is_finite() { member.scale.clamp(0.5, 3.0) } else { 1.0 };
            member.opacity = if member.opacity.is_finite() { member.opacity.clamp(0.1, 1.0) } else { 1.0 };
            member.dx = if member.dx.is_finite() { member.dx.clamp(0.0, 4000.0) } else { 0.0 };
            member.dy = if member.dy.is_finite() { member.dy.clamp(0.0, 4000.0) } else { 0.0 };
            member.options.retain(|name, value| OPTIONS.contains(&name.as_str()) && value.len() <= 60);
            if let Some(zone) = member.options.get("timezone") {
                if zone.parse::<chrono_tz::Tz>().is_err() {
                    member.options.remove("timezone");
                }
            }
        }
        let mut total = 0u64;
        for look in &self.looks {
            if !valid_id(&look.id) {
                return Err("This holds a look with a name that isn't allowed.".into());
            }
            if paperloom_skin::is_built_in(&look.id) {
                return Err(format!("This holds “{}”, a look that ships with Paperloom, and can't replace it.", look.id));
            }
            for (path, encoded) in &look.files {
                if !paperloom_fetch::looks::safe_path(path) {
                    return Err(format!("This holds a file it isn't allowed to have: {path}"));
                }
                total += (encoded.len() as u64) / 4 * 3;
            }
            let skin_text = look.files.get("skin.toml").and_then(|encoded| base64url_decode(encoded)).and_then(|bytes| String::from_utf8(bytes).ok());
            let parsed = skin_text.as_deref().map(paperloom_skin::Skin::parse);
            match parsed {
                Some(Ok(skin)) if skin.id == look.id => {}
                Some(Ok(_)) => return Err("A look in this doesn't match its own name.".into()),
                _ => return Err(format!("The look “{}” in this can't be read.", look.id)),
            }
        }
        if total > MAX_BYTES {
            return Err("This is larger than the 12 MB limit.".into());
        }
        Ok(())
    }

    /// The looks in it that are already on this computer with the same name.
    pub fn looks_already_here(&self) -> Vec<String> {
        self.looks.iter().filter(|look| paperloom_skin::get(&look.id).is_some()).map(|look| look.id.clone()).collect()
    }
}

/// `clock/neon`, `author/name`: letters, digits, `-`, `_`, `.` and one or more `/`, no `..`.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && !id.contains("..") && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// The folder name an id is stored under.
fn folder_of(id: &str) -> String {
    id.replace('/', "-")
}

// --- Building a share from what is on this computer --------------------------

fn member_of(placed: &Placed) -> Member {
    let mut options = placed.options.clone();
    options.retain(|name, _| OPTIONS.contains(&name.as_str()));
    Member { kind: placed.kind.clone(), look: placed.skin.clone(), scale: placed.scale, opacity: placed.opacity, options, dx: placed.dx, dy: placed.dy }
}

/// The files of an installed look, if it isn't one that ships inside Paperloom.
fn files_of(look_id: &str) -> Result<Option<LookFiles>, String> {
    if look_id.is_empty() || paperloom_skin::is_built_in(look_id) {
        return Ok(None);
    }
    let Some(dir) = paperloom_skin::get(look_id).and_then(|skin| skin.dir.clone()) else { return Ok(None) };
    let files = paperloom_fetch::themes::read_folder(&dir)?;
    Ok(Some(LookFiles { id: look_id.to_string(), files: files.into_iter().map(|(path, bytes)| (path, base64url_encode(&bytes))).collect() }))
}

fn with_looks(mut shared: Shared, ids: impl IntoIterator<Item = String>) -> Result<Shared, String> {
    let mut seen = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        if let Some(files) = files_of(&id)? {
            shared.looks.push(files);
        }
        seen.push(id);
    }
    Ok(shared)
}

/// One widget exactly as it is set up.
pub fn of_widget(placed: &Placed) -> Result<Shared, String> {
    let name = widgets::entry(&placed.kind).map_or("Widget", |entry| entry.name).to_string();
    with_looks(Shared { v: 1, name, widgets: vec![member_of(placed)], stack: None, looks: Vec::new() }, [placed.skin.clone()])
}

/// A stack of widgets on the desktop.
pub fn of_stack(all: &widgets::Widgets, stack: u32) -> Result<Shared, String> {
    let info = all.stack(stack).ok_or("That stack isn't there any more.")?;
    let members: Vec<Member> = all.members(stack).map(member_of).collect();
    let ids: Vec<String> = members.iter().map(|member| member.look.clone()).collect();
    with_looks(Shared { v: 1, name: info.name.clone(), widgets: members, stack: Some(info.size), looks: Vec::new() }, ids)
}

// --- Putting a share onto this computer -------------------------------------

/// Writes the looks that came with it into the looks folder, each replacing an
/// older copy of the same look. Returns their ids.
pub fn install_looks(shared: &Shared, skins_dir: &Path) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for look in &shared.looks {
        let folder = folder_of(&look.id);
        let staging = skins_dir.join(format!(".installing-{folder}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|err| err.to_string())?;
        let written = (|| -> Result<(), String> {
            for (path, encoded) in &look.files {
                let bytes = base64url_decode(encoded).ok_or("a file in this is damaged")?;
                let target = staging.join(path);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
                }
                std::fs::write(&target, bytes).map_err(|err| err.to_string())?;
            }
            Ok(())
        })();
        if let Err(why) = written {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(why);
        }
        let live = skins_dir.join(&folder);
        let _ = std::fs::remove_dir_all(&live);
        std::fs::rename(&staging, &live).map_err(|err| {
            let _ = std::fs::remove_dir_all(&staging);
            err.to_string()
        })?;
        done.push(look.id.clone());
    }
    Ok(done)
}

/// Saves a stack as an installed preset (`stacks_dir/<folder>/stack.toml`), so
/// it shows in Widgets → Stacks. Returns the preset's id.
pub fn install_stack(shared: &Shared, stacks_dir: &Path) -> Result<String, String> {
    let size = shared.stack.ok_or("This isn't a stack.")?;
    let slug: String = shared.name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').chars().take(30).collect();
    let slug = if slug.is_empty() { "stack".to_string() } else { slug };
    let existing: Vec<String> = paperloom_common::stacks::presets().into_iter().map(|preset| preset.id).collect();
    let mut id = format!("shared/{slug}");
    let mut number = 2;
    while existing.contains(&id) {
        id = format!("shared/{slug}-{number}");
        number += 1;
    }
    let mut text = format!("id = \"{id}\"\nname = {:?}\nsize = [{}, {}]\n", shared.name, size[0], size[1]);
    for member in &shared.widgets {
        text.push_str(&format!("\n[[widget]]\nkind = {:?}\nskin = {:?}\ndx = {}\ndy = {}\n", member.kind, member.look, member.dx, member.dy));
    }
    let folder = stacks_dir.join(folder_of(&id));
    std::fs::create_dir_all(&folder).map_err(|err| err.to_string())?;
    std::fs::write(folder.join("stack.toml"), text).map_err(|err| err.to_string())?;
    Ok(id)
}

/// Places the shared widget (the first one) on the desktop. The caller saves.
pub fn place(shared: &Shared, all: &mut widgets::Widgets) -> Result<u32, String> {
    let member = shared.widgets.first().ok_or("There is no widget in this.")?;
    let id = all.add(&member.kind).ok_or("There is no room for another widget.")?;
    if let Some(target) = all.placed.iter_mut().find(|target| target.id == id) {
        target.skin.clone_from(&member.look);
        target.scale = member.scale;
        target.opacity = member.opacity;
        target.options = member.options.clone();
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(zone: &str) -> Shared {
        let mut options = BTreeMap::new();
        if !zone.is_empty() {
            options.insert("timezone".to_string(), zone.to_string());
        }
        Shared { v: 1, name: "Clock".into(), widgets: vec![Member { kind: "clock".into(), look: String::new(), scale: 1.5, opacity: 0.8, options, dx: 0.0, dy: 0.0 }], stack: None, looks: Vec::new() }
    }

    #[test]
    fn a_widget_code_round_trips() {
        let code = clock("Asia/Karachi").encode().unwrap();
        assert!(code.starts_with(PREFIX) && code.len() < 400, "{}", code.len());
        let back = Shared::decode(&format!("  {code}\n")).unwrap();
        assert_eq!(back, clock("Asia/Karachi"));
        assert_eq!(back.what(), What::Widget);
    }

    #[test]
    fn strangers_codes_are_cleaned_or_refused() {
        // A made-up time zone and an unknown option are dropped, numbers are clamped.
        let mut odd = clock("Not/AZone");
        odd.widgets[0].options.insert("shell".into(), "rm -rf".into());
        odd.widgets[0].scale = 99.0;
        let back = Shared::decode(&odd.encode().unwrap()).unwrap();
        assert!(back.widgets[0].options.is_empty());
        assert_eq!(back.widgets[0].scale, 3.0);
        // An unknown widget kind, a newer format and garbage are refused.
        let mut unknown = clock("");
        unknown.widgets[0].kind = "teleporter".into();
        assert!(Shared::decode(&unknown.encode().unwrap()).is_err());
        let mut newer = clock("");
        newer.v = 2;
        assert!(Shared::decode(&newer.encode().unwrap()).is_err());
        assert!(Shared::decode("paperloom-widget:1:AAAA").is_err());
        assert!(Shared::decode("hello").is_err());
    }

    #[test]
    fn a_look_cannot_replace_one_that_ships_with_paperloom_or_carry_bad_files() {
        let built_in = paperloom_skin::all().into_iter().find(|skin| paperloom_skin::is_built_in(&skin.id)).unwrap().id.clone();
        let mut bad = Shared { v: 1, name: "x".into(), widgets: Vec::new(), stack: None, looks: vec![LookFiles { id: built_in, files: BTreeMap::new() }] };
        assert!(bad.check().is_err());
        bad.looks = vec![LookFiles { id: "me/look".into(), files: BTreeMap::from([("../evil.toml".to_string(), "AAAA".to_string())]) }];
        assert!(bad.check().is_err());
        bad.looks = vec![LookFiles { id: "me/look".into(), files: BTreeMap::from([("run.exe".to_string(), "AAAA".to_string())]) }];
        assert!(bad.check().is_err());
    }

    #[test]
    fn a_look_with_its_files_installs_and_reads_back() {
        let skin = include_str!("../../../../paperloom-skin/skins/clock-minimal.toml").replace("clock/minimal", "me/minimal");
        let look = LookFiles { id: "me/minimal".into(), files: BTreeMap::from([("skin.toml".to_string(), base64url_encode(skin.as_bytes()))]) };
        let shared = Shared { v: 1, name: "Minimal".into(), widgets: Vec::new(), stack: None, looks: vec![look] };
        let back = Shared::decode(&shared.encode().unwrap()).unwrap();
        assert_eq!(back.what(), What::Look);
        let dir = std::env::temp_dir().join(format!("paperloom-share-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(install_looks(&back, &dir).unwrap(), vec!["me/minimal".to_string()]);
        assert!(dir.join("me-minimal").join("skin.toml").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
