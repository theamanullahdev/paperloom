//! Saved results survive a restart and old entries are pruned. Own process,
//! so it can point the cache folder at a scratch directory first.

use paperloom_fetch::{load_listing, save_listing, OnlineItem, Variant};

fn item(id: &str) -> OnlineItem {
    OnlineItem {
        id: id.into(),
        source: "test".into(),
        source_name: "Test".into(),
        title: format!("Wallpaper {id}"),
        thumb_url: "https://example.test/t.jpg".into(),
        variants: vec![Variant { label: "HD".into(), url: "https://example.test/v.mp4".into() }],
        page_url: String::new(),
    }
}

#[test]
fn saves_loads_and_prunes() {
    let dir = std::env::temp_dir().join(format!("paperloom-cache-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("XDG_CACHE_HOME", &dir);

    assert!(load_listing("rain|a|false").is_none());
    save_listing("rain|a|false", &[item("1"), item("2")]);

    let saved = load_listing("rain|a|false").expect("saved listing");
    assert_eq!(saved.items.len(), 2);
    assert_eq!(saved.items[0], item("1"));
    assert!(saved.age.as_secs() < 5);

    save_listing("empty", &[]);
    assert!(load_listing("empty").is_none(), "empty results are never saved");

    for n in 0..20 {
        save_listing(&format!("query-{n}"), &[item("x")]);
    }
    let kept = (0..20).filter(|n| load_listing(&format!("query-{n}")).is_some()).count();
    assert!(kept <= 16, "kept {kept} entries");
}
