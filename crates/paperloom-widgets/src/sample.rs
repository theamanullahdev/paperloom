//! What the meters show, read once for every widget that needs it. Cheap OS
//! counters only (no process lists), refreshed at most once a second, and not
//! at all while nothing visible is asking.

use std::time::{Duration, Instant};

use paperloom_common::battery::{self, Battery};
use sysinfo::{Disks, Networks, System};

const DISK_EVERY: Duration = Duration::from_secs(30);

pub struct Sample {
    system: System,
    disks: Disks,
    networks: Networks,
    last: Instant,
    last_disks: Instant,
    last_battery: Instant,
    /// Percent of the whole processor.
    pub cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub download: f64,
    pub upload: f64,
    pub battery: Option<Battery>,
    /// Which readings a placed widget shows; the others are never taken.
    needs: Needs,
}

#[derive(Clone, Copy)]
struct Needs {
    cpu: bool,
    memory: bool,
    network: bool,
    disk: bool,
}

impl Sample {
    pub fn new() -> Self {
        let mut sample = Self {
            system: System::new(),
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last: Instant::now() - Duration::from_secs(60),
            last_disks: Instant::now() - DISK_EVERY,
            last_battery: Instant::now() - DISK_EVERY,
            cpu: 0.0,
            memory_used: 0,
            memory_total: 0,
            disk_used: 0,
            disk_total: 0,
            download: 0.0,
            upload: 0.0,
            battery: None,
            needs: Needs { cpu: true, memory: true, network: true, disk: true },
        };
        sample.system.refresh_cpu_usage(); // the first reading is only a baseline
        sample.refresh();
        sample
    }

    /// Takes only the readings the placed widgets show (the battery is always
    /// read: it decides pausing and pace).
    pub fn set_needs<'a>(&mut self, kinds: impl Iterator<Item = &'a str> + Clone) {
        let has = |kind: &str| kinds.clone().any(|placed| placed == kind);
        self.needs = Needs { cpu: has("cpu"), memory: has("memory"), network: has("network"), disk: has("disk") };
    }

    pub fn refresh(&mut self) {
        let elapsed = self.last.elapsed();
        if elapsed < Duration::from_millis(900) {
            return;
        }
        self.last = Instant::now();

        if self.needs.cpu {
            self.system.refresh_cpu_usage();
            self.cpu = self.system.global_cpu_usage();
        }
        if self.needs.memory {
            self.system.refresh_memory();
            self.memory_used = self.system.used_memory();
            self.memory_total = self.system.total_memory();
        }

        if self.needs.network {
        self.networks.refresh(true);
        let (received, sent) = self
            .networks
            .iter()
            .filter(|(name, _)| !name.starts_with("lo"))
            .fold((0u64, 0u64), |(r, t), (_, data)| (r + data.received(), t + data.transmitted()));
        let seconds = elapsed.as_secs_f64().clamp(0.5, 60.0);
        self.download = received as f64 / seconds;
        self.upload = sent as f64 / seconds;
        }

        if self.needs.disk && self.last_disks.elapsed() >= DISK_EVERY {
            self.last_disks = Instant::now();
            self.disks.refresh(true);
            // The biggest disk stands for "the disk".
            if let Some(main) = self.disks.list().iter().max_by_key(|disk| disk.total_space()) {
                self.disk_total = main.total_space();
                self.disk_used = main.total_space().saturating_sub(main.available_space());
            }
        }
        if self.last_battery.elapsed() >= DISK_EVERY {
            self.last_battery = Instant::now();
            self.battery = battery::read();
        }
    }
}
