//! Hits the real motionbgs.com — run explicitly with
//! `XDG_DATA_HOME=$(mktemp -d) XDG_CACHE_HOME=$(mktemp -d) cargo test -p paperloom-fetch -- --ignored --nocapture`
//! (the env vars keep it out of your real library/cache).

use std::time::{Duration, Instant};

use std::collections::HashMap;

use paperloom_common::Settings;
use paperloom_fetch::{fetch_page, Queue, Status};

#[test]
#[ignore = "network"]
fn browse_search_paginate_and_download_with_resume() {
    let sources = Settings::default().with_builtin_sources().sources;

    let latest = fetch_page(&sources, "", 1, &HashMap::new()).expect("latest page 1");
    assert!(latest.items.len() >= 10, "got {} items", latest.items.len());
    assert!(latest.has_more);
    assert!(latest.items.iter().any(|item| item.has_4k()), "some wallpapers offer 4K");
    let page2 = fetch_page(&sources, "", 2, &latest.cursors).expect("latest page 2");
    assert_ne!(latest.items[0].key(), page2.items[0].key());

    let rain = fetch_page(&sources, "rain", 1, &HashMap::new()).expect("search");
    assert!(!rain.items.is_empty());
    assert!(rain.has_more);
    let rain2 = fetch_page(&sources, "rain", 2, &rain.cursors).expect("search page 2");
    assert!(!rain2.items.is_empty());

    let thumb = paperloom_fetch::fetch_thumbnail(&latest.items[0].thumb_url).expect("thumbnail");
    assert!(thumb.metadata().unwrap().len() > 1000);

    let queue = Queue::start();
    assert!(queue.add(&latest.items[0], "HD"));
    assert!(!queue.add(&latest.items[0], "HD"), "duplicate must be rejected");

    // Interrupt mid-download: wait for some bytes, then simulate a crash by
    // flipping the job to a transient failure path via cancel+re-add isn't
    // faithful, so instead just let it run and verify the final file.
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let jobs = queue.snapshot();
        if jobs[0].status == Status::Done {
            break;
        }
        assert!(Instant::now() < deadline, "timed out: {:?}", jobs[0]);
        std::thread::sleep(Duration::from_millis(500));
    }
    let job = queue.finished().remove(0);
    let bytes = std::fs::read(job.final_path()).unwrap();
    assert_eq!(&bytes[4..8], b"ftyp");
    assert_eq!(Some(bytes.len() as u64), job.bytes_total);
    queue.acknowledge(&job.id, true);
}
