//! Time recording utilities for performance profiling
//!
//! This module provides a simple RAII-style time recorder that automatically
//! tracks and accumulates execution time in named slots.

use alloc::{string::{String, ToString}, vec::Vec};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

struct TimeTracker {
    times: HashMap<String, u128>,
    last_dumped: Instant,
    start_time: Instant,
}

// simple global timing map: auto-creates entries on first record
static COVERAGE_STAGE_TIMES: OnceLock<Mutex<TimeTracker>> = OnceLock::new();

fn get_time_tracker() -> &'static Mutex<TimeTracker> {
    COVERAGE_STAGE_TIMES.get_or_init(|| {
        Mutex::new(TimeTracker {
            times: HashMap::new(),
            last_dumped: Instant::now(),
            start_time: Instant::now(),
        })
    })
}

/// Dump total time snapshot to `/tmp/symcts_times_snapshot.txt`
pub fn dump_total_time_snapshot() {
    let tracker = get_time_tracker().lock().unwrap();
    let snapshot_path = "/tmp/symcts_times_snapshot.txt";
    let mut snapshot_content = String::new();

    // Sort entries by key for consistent output
    let mut entries: Vec<_> = tracker.times.iter().collect();
    entries.sort_by_key(|(k, _)| *k);

    for (slot_name, total_ns) in entries {
        snapshot_content.push_str(&format!("{}: {}\n", slot_name, total_ns))
    }
    snapshot_content.push_str(&format!("total_time: {}\n", tracker.start_time.elapsed().as_nanos()));
    std::fs::write(snapshot_path, &snapshot_content).unwrap();
    eprintln!("Final time snapshot written to {}:\n{}", snapshot_path, snapshot_content);
}

/// Dump percentage time snapshot to `/tmp/symcts_times_snapshot_percentages.txt`
pub fn dump_percentage_time_snapshot() {
    let tracker = get_time_tracker().lock().unwrap();
    let total_time_ns: u128 = tracker.times.values().sum();
    let snapshot_path = "/tmp/symcts_times_snapshot_percentages.txt";
    let mut snapshot_content = String::new();

    // Sort entries by key for consistent output
    let mut entries: Vec<_> = tracker.times.iter().collect();
    entries.sort_by_key(|(k, _)| *k);

    for (slot_name, total_ns) in entries {
        let percentage = (*total_ns as f64 / total_time_ns as f64) * 100.0;
        snapshot_content.push_str(&format!("{}: {}\n", slot_name, percentage))
    }
    snapshot_content.push_str(&format!("total_time: {}\n", total_time_ns));
    std::fs::write(snapshot_path, &snapshot_content).unwrap();
    eprintln!("Final percentage time snapshot written to {}:\n{}", snapshot_path, snapshot_content);
}

/// Record elapsed time `duration` into the named slot (accumulated in ns).
/// Creates the slot if it does not yet exist and dumps a snapshot to /tmp/symcts_times_snapshot.txt and stderr.
pub fn add_time_for_slot(name: &str, elapsed: Duration) {
    let mut tracker = get_time_tracker().lock().unwrap();
    let entry = tracker.times.entry(name.to_string()).or_insert(0);
    *entry += elapsed.as_nanos();

    // Dump snapshot if more than 10 seconds passed since last dump
    if tracker.last_dumped.elapsed() > Duration::from_secs(10) {
        tracker.last_dumped = Instant::now();
        drop(tracker); // release lock
        dump_total_time_snapshot();
        dump_percentage_time_snapshot();
    }
}

/// A simple RAII object that measures elapsed time and records it into a named slot on drop
pub struct TimeRecorder {
    name: String,
    start: Instant,
}

impl TimeRecorder {
    /// Create a new time recorder for the given named slot
    pub fn new(name: &str) -> Self {
        TimeRecorder {
            name: name.to_string(),
            start: Instant::now(),
        }
    }

    /// Manually record the time elapsed so far (without dropping)
    pub fn record_time(&self) {
        add_time_for_slot(&self.name, self.start.elapsed());
    }

    /// Get the elapsed time since creation
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for TimeRecorder {
    fn drop(&mut self) {
        self.record_time();
    }
}
