//! A few minutes of Paperloom's own usage, kept only so the Usage and
//! Downloads graphs are already filled when those pages open.
//!
//! Deliberately tiny: every two seconds it re-reads just Paperloom's own
//! handful of processes (a few `/proc` reads) plus one atomic counter, and
//! rediscovers them with a full scan only every 30 seconds. The history is
//! 150 numbers per series — a few kilobytes.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub const EVERY: Duration = Duration::from_secs(2);
/// Five minutes at one sample per `EVERY`.
pub const WINDOW: usize = 150;
/// A full process scan happens once per this many samples.
const RESCAN_EVERY: u32 = 15;

#[derive(Default)]
pub struct History {
    pub cpu_app: VecDeque<f32>,
    pub cpu_engine: VecDeque<f32>,
    pub memory_app: VecDeque<f32>,
    pub memory_engine: VecDeque<f32>,
    pub network: VecDeque<f32>,
    pub disk_read: VecDeque<f32>,
    pub disk_write: VecDeque<f32>,
}

impl History {
    pub fn series(values: &VecDeque<f32>) -> Vec<f32> {
        values.iter().copied().collect()
    }
}

pub struct Recorder {
    system: System,
    cores: f32,
    /// Paperloom's processes, and whether each is the wallpaper engine.
    known: Vec<(Pid, bool)>,
    ticks: u32,
    last: Instant,
    last_network: u64,
    pub history: History,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            cores: std::thread::available_parallelism().map_or(1.0, |n| n.get() as f32),
            known: Vec::new(),
            ticks: 0,
            last: Instant::now(),
            last_network: paperloom_fetch::bytes_received(),
            history: History::default(),
        }
    }

    pub fn tick(&mut self) {
        let elapsed = self.last.elapsed().as_secs_f32().max(0.2);
        self.last = Instant::now();
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage();

        if self.ticks.is_multiple_of(RESCAN_EVERY) || self.known.is_empty() {
            self.system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
            self.known = self.scan();
        } else {
            let pids: Vec<Pid> = self.known.iter().map(|(pid, _)| *pid).collect();
            self.system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, kind);
        }
        self.ticks = self.ticks.wrapping_add(1);

        let (mut cpu, mut memory) = ([0.0f32; 2], [0.0f32; 2]);
        let (mut read, mut write) = (0.0f32, 0.0f32);
        for (pid, engine) in &self.known {
            let Some(process) = self.system.process(*pid) else { continue };
            let slot = usize::from(*engine);
            cpu[slot] += process.cpu_usage() / self.cores;
            memory[slot] += process.memory() as f32;
            let disk = process.disk_usage();
            read += disk.read_bytes as f32 / elapsed;
            write += disk.written_bytes as f32 / elapsed;
        }
        let total = paperloom_fetch::bytes_received();
        let network = total.saturating_sub(self.last_network) as f32 / elapsed;
        self.last_network = total;

        let history = &mut self.history;
        push(&mut history.cpu_app, cpu[0]);
        push(&mut history.cpu_engine, cpu[1]);
        push(&mut history.memory_app, memory[0]);
        push(&mut history.memory_engine, memory[1]);
        push(&mut history.network, network);
        push(&mut history.disk_read, read);
        push(&mut history.disk_write, write);
    }

    fn scan(&self) -> Vec<(Pid, bool)> {
        let me = Pid::from_u32(std::process::id());
        let ours = |pid: Pid| self.system.process(pid).is_some_and(|p| p.name().to_string_lossy().to_lowercase().starts_with("paperloom"));
        let mut found = Vec::new();
        for (pid, process) in self.system.processes() {
            if process.thread_kind().is_some() {
                continue;
            }
            let lower = process.name().to_string_lossy().to_lowercase();
            if *pid == me {
                found.push((*pid, false));
            } else if lower.starts_with("paperloom-engin") {
                found.push((*pid, true));
            } else if lower.starts_with("paperloom") || (lower.starts_with("ffmpeg") && process.parent().is_some_and(ours)) {
                found.push((*pid, false));
            }
        }
        found
    }
}

fn push(series: &mut VecDeque<f32>, value: f32) {
    if series.len() == WINDOW {
        series.pop_front();
    }
    series.push_back(value);
}
