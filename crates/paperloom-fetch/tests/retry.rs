//! The queue's core promise: a download that hits transient failures keeps
//! its reference and keeps retrying until it succeeds. Uses a local server
//! that fails the first two requests (503, then a dropped connection) and
//! serves the file on the third.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use paperloom_fetch::{OnlineItem, Queue, Status};

#[test]
fn transient_failures_retry_until_success() {
    let data = tempdir("retry-data");
    // Paths are resolved from XDG_DATA_HOME on first use, so this must be
    // set before anything touches them (this test file is its own process).
    std::env::set_var("XDG_DATA_HOME", &data);

    let mut body = vec![7u8; 100_000];
    body[4..8].copy_from_slice(b"ftyp");
    let served = body.clone();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v.mp4", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for (attempt, incoming) in listener.incoming().enumerate() {
            let mut stream = incoming.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            match attempt {
                0 => stream.write_all(b"HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap(),
                1 => {} // hang up without answering
                _ => {
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", served.len());
                    stream.write_all(head.as_bytes()).unwrap();
                    stream.write_all(&served).unwrap();
                }
            }
        }
    });

    let queue = Queue::start();
    let item = OnlineItem {
        id: "1".into(),
        source: "test",
        title: "Retry Me".into(),
        quality: String::new(),
        thumb_url: String::new(),
        download_url: url,
        page_url: String::new(),
    };
    assert!(queue.add(&item));

    let mut saw_waiting = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let job = queue.snapshot().remove(0);
        saw_waiting |= job.status == Status::Waiting;
        if job.status == Status::Done {
            assert_eq!(job.attempts, 2, "two failures before success");
            assert_eq!(std::fs::read(job.final_path()).unwrap(), body);
            break;
        }
        assert_ne!(job.status, Status::Failed, "transient errors must never become permanent: {job:?}");
        assert!(Instant::now() < deadline, "timed out: {job:?}");
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(saw_waiting, "job should have been Waiting between attempts");
    assert_eq!(queue.snapshot().len(), 1, "reference kept the whole time");
}

fn tempdir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("paperloom-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
