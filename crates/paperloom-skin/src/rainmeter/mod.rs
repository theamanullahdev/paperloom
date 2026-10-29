//! Importing Rainmeter skins: an `.rmskin` package, a skin folder, or one
//! `.ini`. Each Rainmeter skin (one `.ini`) becomes one Paperloom look in the
//! user's looks folder, with its pictures and fonts copied beside it.
//!
//! Only `.ini`, `.inc`, `.png`, `.ttf` and `.otf` files are ever read from a
//! package; nothing in it is run, and nothing is written outside the looks
//! folder.

mod convert;
mod ini;

use std::io::Read;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

/// What an import did.
#[derive(Debug, Default)]
pub struct Report {
    /// The names of the looks it added.
    pub looks: Vec<String>,
    /// Parts of the skins Paperloom couldn't carry over, in words.
    pub left_out: Vec<String>,
}

const MAX_PACKAGE: u64 = 100 * 1024 * 1024;
const MAX_FILES: usize = 4000;
const KEPT: [&str; 5] = ["ini", "inc", "png", "ttf", "otf"];

/// Imports `path` (an `.rmskin`/`.zip`, a folder, or an `.ini`) into `skins_dir`.
pub fn import(path: &Path, skins_dir: &Path) -> Result<Report, String> {
    std::fs::create_dir_all(skins_dir).map_err(|err| err.to_string())?;
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    if path.is_dir() {
        let suites = suites_in(path);
        return import_suites(&suites, skins_dir, None);
    }
    match extension.as_str() {
        "rmskin" | "zip" => {
            let staging = skins_dir.join(format!(".unpacking-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&staging);
            let result = unpack(path, &staging).and_then(|author| import_suites(&suites_in(&staging), skins_dir, author));
            let _ = std::fs::remove_dir_all(&staging);
            result
        }
        "ini" => {
            let root = suite_root(path.parent().unwrap_or(Path::new(".")));
            let mut report = Report::default();
            import_ini(path, &root, skins_dir, None, &mut report)?;
            Ok(report)
        }
        _ => Err("Pick a Rainmeter skin: an .rmskin file, a skin folder, or its .ini file.".into()),
    }
}

/// The skin suites under `dir`: `Skins/<Suite>` in a package, else `dir` itself.
fn suites_in(dir: &Path) -> Vec<PathBuf> {
    let skins = ["Skins", "skins"].iter().map(|name| dir.join(name)).find(|path| path.is_dir());
    match skins {
        Some(skins) => std::fs::read_dir(skins).map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect()).unwrap_or_default(),
        None => vec![suite_root(dir)],
    }
}

/// The folder that holds a skin's `@Resources` (a skin may sit a level or two below it).
fn suite_root(dir: &Path) -> PathBuf {
    let mut current = dir;
    for _ in 0..3 {
        if current.join("@Resources").is_dir() {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    dir.to_path_buf()
}

fn import_suites(suites: &[PathBuf], skins_dir: &Path, author: Option<String>) -> Result<Report, String> {
    let mut report = Report::default();
    for suite in suites {
        for ini in ini_files(suite, 0) {
            if let Err(err) = import_ini(&ini, suite, skins_dir, author.clone(), &mut report) {
                report.left_out.push(err);
            }
        }
    }
    if report.looks.is_empty() {
        return Err(if report.left_out.is_empty() { "No Rainmeter skins were found there.".into() } else { format!("Nothing could be carried over: {}", report.left_out.join("; ")) });
    }
    Ok(report)
}

fn ini_files(dir: &Path, depth: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return found };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if path.is_dir() {
            if depth < 3 && !name.starts_with('@') {
                found.extend(ini_files(&path, depth + 1));
            }
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ini")) {
            found.push(path);
        }
    }
    found
}

fn import_ini(path: &Path, root: &Path, skins_dir: &Path, author: Option<String>, report: &mut Report) -> Result<(), String> {
    let resources = root.join("@Resources");
    let parsed = ini::Ini::load(path, &resources)?;
    let skin_dir = path.parent().unwrap_or(root);
    let converted = convert::convert(&parsed, skin_dir, root);
    if converted.elements.is_empty() {
        return Ok(()); // A settings or variables file, not something drawn.
    }

    let suite = root.file_name().and_then(|name| name.to_str()).unwrap_or("Rainmeter").to_string();
    let relative = path.strip_prefix(root).unwrap_or(path).with_extension("");
    let parts: Vec<String> = relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    // "Mond / Time / Time" reads as "Mond Time".
    let mut words = vec![suite.clone()];
    for part in parts {
        if !words.iter().any(|word| word.eq_ignore_ascii_case(&part)) {
            words.push(part);
        }
    }
    let name = words.join(" ");
    let folder: String = format!("rm-{name}").chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>().split('-').filter(|piece| !piece.is_empty()).collect::<Vec<_>>().join("-");

    let metadata = parsed.section("Metadata");
    let author = metadata.and_then(|section| section.get("author")).map(str::to_string).or(author).unwrap_or_else(|| "Rainmeter".into());
    let about = metadata.and_then(|section| section.get("information")).unwrap_or("Imported from Rainmeter.");

    let staging = skins_dir.join(format!(".importing-{folder}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("images")).map_err(|err| err.to_string())?;
    let written = (|| -> Result<(), String> {
        for (from, to) in &converted.images {
            std::fs::copy(from, staging.join(to)).map_err(|err| err.to_string())?;
        }
        // Rainmeter loads every font in @Resources/Fonts; so does the look.
        let mut fonts = Vec::new();
        let uses_fonts = converted.elements.iter().any(|element| element.contains_key("font"));
        if let (true, Ok(entries)) = (uses_fonts, std::fs::read_dir(resources.join("Fonts"))) {
            std::fs::create_dir_all(staging.join("fonts")).map_err(|err| err.to_string())?;
            for entry in entries.flatten() {
                let path = entry.path();
                let font = path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf"));
                let small = entry.metadata().is_ok_and(|meta| meta.len() < 10 * 1024 * 1024);
                if font && small && fonts.len() < 16 {
                    let file = format!("fonts/{}", path.file_name().and_then(|name| name.to_str()).unwrap_or("font.ttf"));
                    std::fs::copy(&path, staging.join(&file)).map_err(|err| err.to_string())?;
                    fonts.push(Value::String(file));
                }
            }
        }

        let mut look = Table::new();
        look.insert("id".into(), format!("{}/{folder}", converted.kind).into());
        look.insert("name".into(), name.clone().into());
        look.insert("author".into(), author.into());
        look.insert("description".into(), about.chars().take(200).collect::<String>().into());
        look.insert("kinds".into(), Value::Array(vec![converted.kind.clone().into()]));
        look.insert("fonts".into(), Value::Array(fonts));
        look.insert("size".into(), Value::Array(vec![f64::from(converted.size.0).into(), f64::from(converted.size.1).into()]));
        look.insert("element".into(), Value::Array(converted.elements.iter().cloned().map(Value::Table).collect()));
        let source = toml::to_string(&look).map_err(|err| err.to_string())?;
        // Never install something the renderer can't read.
        crate::Skin::parse(&source).map_err(|err| format!("{name}: {err}"))?;
        std::fs::write(staging.join("skin.toml"), source).map_err(|err| err.to_string())?;
        let final_dir = skins_dir.join(&folder);
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(&staging, &final_dir).map_err(|err| err.to_string())
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }
    report.looks.push(name.clone());
    report.left_out.extend(converted.notes.into_iter().map(|note| format!("{name} — {note}")));
    Ok(())
}

/// Unpacks the files the importer reads from an `.rmskin` into `into`, and
/// returns the package's author if it names one.
fn unpack(path: &Path, into: &Path) -> Result<Option<String>, String> {
    let size = std::fs::metadata(path).map_err(|err| err.to_string())?.len();
    if size > MAX_PACKAGE {
        return Err("That package is too big to be a skin.".into());
    }
    let mut bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    // An .rmskin is a zip with a small footer after it.
    if bytes.ends_with(b"RMSKIN\0") && bytes.len() > 16 {
        bytes.truncate(bytes.len() - 16);
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "That file isn't a Rainmeter skin package.".to_string())?;
    if archive.len() > MAX_FILES {
        return Err("That package has too many files to be a skin.".into());
    }
    let mut total = 0u64;
    let mut author = None;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| err.to_string())?;
        // Only plain relative paths: nothing can land outside `into`.
        let Some(relative) = entry.enclosed_name() else { continue };
        let keep = relative.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| KEPT.contains(&ext.to_ascii_lowercase().as_str()));
        if entry.is_dir() || !keep {
            continue;
        }
        total += entry.size();
        if total > MAX_PACKAGE * 2 {
            return Err("That package unpacks to too much to be a skin.".into());
        }
        let mut contents = Vec::new();
        entry.by_ref().take(MAX_PACKAGE).read_to_end(&mut contents).map_err(|err| err.to_string())?;
        if relative.as_os_str().eq_ignore_ascii_case("RMSKIN.ini") {
            author = String::from_utf8_lossy(&contents).lines().find_map(|line| line.trim().strip_prefix("Author=").map(|value| value.trim().to_string()));
        }
        let target = into.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        std::fs::write(target, contents).map_err(|err| err.to_string())?;
    }
    Ok(author)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_a_skin_folder() {
        let base = std::env::temp_dir().join(format!("paperloom-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let suite = base.join("Mondish");
        std::fs::create_dir_all(suite.join("@Resources")).unwrap();
        std::fs::create_dir_all(suite.join("Time")).unwrap();
        std::fs::write(suite.join("@Resources/vars.inc"), "[Variables]\nColor=255,255,255,230\nSize=40\n").unwrap();
        std::fs::write(
            suite.join("Time/Time.ini"),
            "[Rainmeter]\nUpdate=1000\n@Include=#@#vars.inc\n[Metadata]\nAuthor=Someone\n\
             [MeasureTime]\nMeasure=Time\nFormat=%H:%M\n[MeasureDay]\nMeasure=Time\nFormat=%A\n\
             [MeasureCPU]\nMeasure=CPU\n[MeasureWeather]\nMeasure=WebParser\n\
             [Style]\nFontColor=#Color#\nAntiAlias=1\n\
             [Clock]\nMeter=String\nMeterStyle=Style\nMeasureName=MeasureTime\nFontSize=#Size#\nStringAlign=Center\nX=(#Size#*3)\nY=0\nStringCase=Upper\n\
             [Day]\nMeter=String\nMeterStyle=Style\nMeasureName=MeasureDay\nText=It's %1\nX=0r\nY=10R\nStringAlign=Center\n\
             [Line]\nMeter=Shape\nShape=Rectangle 0,0,200,2 | Fill Color #Color# | StrokeWidth 0\nY=4R\n",
        )
        .unwrap();
        let looks = base.join("looks");
        let report = import(&suite, &looks).expect("imports");
        assert_eq!(report.looks, vec!["Mondish Time".to_string()]);
        assert!(report.left_out.is_empty(), "WebParser isn't shown by any meter, so nothing is missing: {:?}", report.left_out);
        let source = std::fs::read_to_string(looks.join("rm-mondish-time/skin.toml")).unwrap();
        let skin = crate::Skin::parse(&source).unwrap();
        assert_eq!(skin.kinds, vec!["clock".to_string()]);
        assert_eq!(skin.author, "Someone");
        assert!(source.contains("{%H:%M}") && source.contains("It's {%A}"), "{source}");
        assert!(source.contains("#ffffffe6"), "{source}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
