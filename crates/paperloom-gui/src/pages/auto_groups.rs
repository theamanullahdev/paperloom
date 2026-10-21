//! "Auto" collections: the library sorted into groups by what each
//! wallpaper's title is about (nature, space, anime…), where it came from,
//! and when it was added. Computed on the fly from titles already on disk —
//! no service, no account, nothing stored. A group can be saved as a real
//! (custom) collection with one click.

use paperloom_common::LibraryEntry;

pub struct AutoGroup {
    pub name: String,
    pub members: Vec<u64>,
}

/// Topic → title words that put a wallpaper in it.
const TOPICS: &[(&str, &[&str])] = &[
    ("Nature", &["nature", "forest", "tree", "mountain", "waterfall", "river", "lake", "landscape", "jungle", "woods", "flower", "leaf", "leaves", "valley", "meadow", "garden", "canyon", "desert"]),
    ("Ocean & beach", &["ocean", "sea", "beach", "wave", "underwater", "island", "tropical", "coast", "shore", "reef"]),
    ("Space", &["space", "galaxy", "planet", "star", "nebula", "astronaut", "cosmos", "universe", "moon", "earth", "cosmic", "milky"]),
    ("City & night", &["city", "skyline", "street", "tokyo", "night", "neon", "urban", "downtown", "tower", "bridge", "traffic"]),
    ("Anime & Japan", &["anime", "manga", "japan", "japanese", "samurai", "ninja", "katana", "sakura", "cherry", "ghibli", "kimono"]),
    ("Cars & racing", &["car", "racing", "supercar", "drift", "jdm", "formula", "motorsport", "motorcycle", "bike", "porsche", "ferrari", "lamborghini", "bmw"]),
    ("Games", &["game", "gaming", "minecraft", "fortnite", "valorant", "gta", "esports", "pixel", "arcade"]),
    ("Fantasy & sci-fi", &["fantasy", "dragon", "castle", "magic", "medieval", "futuristic", "robot", "cyberpunk", "alien", "mech", "wizard", "knight"]),
    ("Abstract", &["abstract", "geometric", "fluid", "particles", "minimal", "gradient", "pattern", "loop", "shapes", "waves", "swirl", "smoke", "glitch"]),
    ("Animals", &["cat", "dog", "animal", "wildlife", "bird", "horse", "wolf", "lion", "tiger", "fox", "butterfly", "fish", "deer"]),
    ("Weather & seasons", &["rain", "snow", "winter", "autumn", "fall", "storm", "lightning", "cloud", "sunset", "sunrise", "fog", "spring", "summer", "aurora"]),
    ("Cozy & lofi", &["cozy", "lofi", "fireplace", "study", "coffee", "candle", "room", "cafe"]),
];

fn source_name(source_id: &str) -> Option<&'static str> {
    let site = source_id.split(':').next()?;
    Some(match site {
        s if s.starts_with("motionbgs") => "From MotionBGS",
        s if s.starts_with("pexels") => "From Pexels",
        s if s.starts_with("pixabay") => "From Pixabay",
        s if s.starts_with("mixkit") => "From Mixkit",
        s if s.starts_with("wallpaperwaves") => "From Wallpaper Waves",
        _ => return None,
    })
}

fn words(title: &str) -> Vec<String> {
    title.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_string).collect()
}

fn matches(words: &[String], keywords: &[&str]) -> bool {
    words.iter().any(|word| keywords.iter().any(|keyword| word == keyword || word.strip_suffix('s') == Some(keyword) || word.strip_suffix("es") == Some(keyword)))
}

/// The non-empty groups for these (visible) wallpapers, biggest topics
/// first, then sources, then "Recently added".
pub fn groups(entries: &[&LibraryEntry]) -> Vec<AutoGroup> {
    let mut topics: Vec<AutoGroup> = TOPICS.iter().map(|(name, _)| AutoGroup { name: (*name).to_string(), members: Vec::new() }).collect();
    let mut other = AutoGroup { name: "Everything else".into(), members: Vec::new() };
    let mut sources: Vec<AutoGroup> = Vec::new();
    let mut imported = AutoGroup { name: "Imported by you".into(), members: Vec::new() };

    for entry in entries {
        let title = words(&entry.display_name);
        let mut placed = false;
        for (index, (_, keywords)) in TOPICS.iter().enumerate() {
            if matches(&title, keywords) {
                topics[index].members.push(entry.id);
                placed = true;
            }
        }
        if !placed {
            other.members.push(entry.id);
        }
        match entry.source_id.as_deref().and_then(source_name) {
            Some(name) => match sources.iter_mut().find(|group| group.name == name) {
                Some(group) => group.members.push(entry.id),
                None => sources.push(AutoGroup { name: name.to_string(), members: vec![entry.id] }),
            },
            None => imported.members.push(entry.id),
        }
    }

    topics.retain(|group| !group.members.is_empty());
    topics.sort_by_key(|group| std::cmp::Reverse(group.members.len()));
    sources.sort_by(|a, b| a.name.cmp(&b.name));

    let mut recent: Vec<&&LibraryEntry> = entries.iter().collect();
    recent.sort_by_key(|entry| std::cmp::Reverse((entry.added, entry.id)));
    let recent = AutoGroup { name: "Recently added".into(), members: recent.into_iter().take(12).map(|entry| entry.id).collect() };

    let mut all = Vec::new();
    all.extend(topics);
    if !other.members.is_empty() && !all.is_empty() {
        all.push(other);
    }
    all.extend(sources);
    if !imported.members.is_empty() {
        all.push(imported);
    }
    if !recent.members.is_empty() {
        all.push(recent);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, name: &str, source: Option<&str>) -> LibraryEntry {
        LibraryEntry {
            id,
            display_name: name.into(),
            path: Default::default(),
            mode: paperloom_common::ImportMode::ReferenceInPlace,
            thumbnail: None,
            preview_frames: Vec::new(),
            preview_version: 0,
            source_id: source.map(Into::into),
            hidden: false,
            added: id,
            optimize: Default::default(),
            fingerprint: None,
        }
    }

    #[test]
    fn sorts_by_title_and_source() {
        let (a, b, c) = (entry(1, "Misty Mountains at dawn", Some("motionbgs.com:1")), entry(2, "Neon Tokyo Streets", None), entry(3, "Untitled", None));
        let groups = groups(&[&a, &b, &c]);
        let find = |name: &str| groups.iter().find(|group| group.name == name).map(|group| group.members.clone());
        assert_eq!(find("Nature"), Some(vec![1]));
        assert_eq!(find("City & night"), Some(vec![2]));
        assert_eq!(find("Everything else"), Some(vec![3]));
        assert_eq!(find("From MotionBGS"), Some(vec![1]));
        assert_eq!(find("Imported by you"), Some(vec![2, 3]));
    }
}
