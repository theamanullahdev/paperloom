//! Search-keyword helpers for the online library: related keywords (so one
//! search isn't a handful of results — the Online page also searches a few
//! related terms and merges everything), and the 18+ check behind the
//! "Allow 18+ content" setting.

/// Topic → related search terms. Matched against every word of the query
/// (and its singular form), so "anime girl" picks up both lists.
const RELATED: &[(&str, &[&str])] = &[
    ("anime", &["manga", "japanese", "cyberpunk anime", "lofi", "studio ghibli"]),
    ("car", &["cars", "racing", "supercar", "drift", "jdm"]),
    ("racing", &["cars", "formula 1", "drift", "motorsport"]),
    ("nature", &["forest", "mountains", "waterfall", "landscape", "river"]),
    ("forest", &["trees", "nature", "jungle", "woods"]),
    ("ocean", &["sea", "waves", "beach", "underwater"]),
    ("sea", &["ocean", "waves", "beach"]),
    ("beach", &["ocean", "sunset", "tropical", "waves"]),
    ("space", &["galaxy", "planet", "stars", "nebula", "astronaut"]),
    ("galaxy", &["space", "nebula", "stars", "universe"]),
    ("city", &["night city", "skyline", "street", "neon city", "tokyo"]),
    ("cyberpunk", &["neon", "futuristic city", "sci-fi", "night city"]),
    ("neon", &["cyberpunk", "synthwave", "night city", "lights"]),
    ("game", &["gaming", "video game", "minecraft", "fantasy"]),
    ("gaming", &["game", "video game", "esports", "rgb"]),
    ("rain", &["rainy", "storm", "window rain", "cozy"]),
    ("snow", &["winter", "snowfall", "christmas", "mountains"]),
    ("winter", &["snow", "snowfall", "christmas", "cozy"]),
    ("sunset", &["sunrise", "beach", "evening", "sky"]),
    ("sky", &["clouds", "sunset", "stars"]),
    ("fantasy", &["dragon", "castle", "magic", "medieval"]),
    ("dragon", &["fantasy", "creature", "fire"]),
    ("cat", &["cats", "kitten", "animal"]),
    ("dog", &["dogs", "puppy", "animal"]),
    ("animal", &["wildlife", "cat", "dog", "bird"]),
    ("abstract", &["geometric", "fluid", "particles", "minimal"]),
    ("minimal", &["minimalist", "abstract", "simple"]),
    ("lofi", &["cozy", "anime", "study", "rain"]),
    ("cozy", &["lofi", "rain", "fireplace", "autumn"]),
    ("autumn", &["fall", "leaves", "cozy", "forest"]),
    ("samurai", &["japan", "ninja", "katana", "warrior"]),
    ("japan", &["japanese", "tokyo", "samurai", "cherry blossom"]),
    ("waterfall", &["nature", "river", "forest"]),
    ("mountain", &["mountains", "nature", "snow", "landscape"]),
    ("fire", &["flames", "fireplace", "lava"]),
    ("music", &["audio visualizer", "synthwave", "lofi"]),
    ("synthwave", &["retrowave", "neon", "outrun", "80s"]),
    ("tech", &["technology", "code", "matrix", "circuit"]),
    ("matrix", &["code", "hacker", "digital rain"]),
];

/// Up to `max` further searches that should broaden `query`: the separate
/// words of a multi-word query first, then related topics. Never includes
/// the query itself or duplicates; empty for an empty query (browsing).
pub fn related_queries(query: &str, max: usize) -> Vec<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let words: Vec<&str> = query.split_whitespace().filter(|word| word.len() > 2).collect();

    let mut out: Vec<String> = Vec::new();
    let push = |candidate: &str, out: &mut Vec<String>| {
        let candidate = candidate.trim().to_lowercase();
        if !candidate.is_empty() && candidate != query && !out.contains(&candidate) {
            out.push(candidate);
        }
    };
    if words.len() > 1 {
        for word in &words {
            push(word, &mut out);
        }
    }
    for word in &words {
        let singular = word.strip_suffix('s').unwrap_or(word);
        for (topic, related) in RELATED {
            if *topic == *word || *topic == singular {
                for term in *related {
                    push(term, &mut out);
                }
            }
        }
    }
    out.truncate(max);
    out
}

/// Words that mark a search or a title as adult content. Deliberately a
/// plain whole-word list: it gates what the app searches for and shows by
/// default, it doesn't classify videos.
const MATURE: &[&str] = &[
    "nsfw", "18+", "adult", "nude", "nudes", "naked", "sexy", "sex", "erotic", "hentai", "ecchi", "porn", "lewd", "xxx",
    "lingerie", "bikini", "boobs", "ass", "onlyfans", "fetish", "topless",
];

/// Whether `text` contains a mature keyword as a whole word.
pub fn is_mature(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '+'))
        .any(|word| MATURE.contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_single_and_multi_word_queries() {
        let car = related_queries("Cars", 4);
        assert_eq!(car, ["racing", "supercar", "drift", "jdm"]);
        let both = related_queries("anime city", 6);
        assert_eq!(&both[..2], ["anime", "city"]);
        assert!(both.contains(&"manga".to_string()));
        assert!(related_queries("", 5).is_empty());
        assert!(related_queries("zzqx", 5).is_empty());
        assert!(!related_queries("ocean", 10).contains(&"ocean".to_string()));
    }

    #[test]
    fn spots_mature_words_only_as_whole_words() {
        assert!(is_mature("sexy anime"));
        assert!(is_mature("NSFW wallpaper"));
        assert!(is_mature("anime 18+"));
        assert!(!is_mature("Sussex countryside"));
        assert!(!is_mature("passenger train"));
        assert!(!is_mature("glass city"));
    }
}
