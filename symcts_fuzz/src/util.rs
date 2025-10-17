// use libafl::{inputs::BytesInput, executors::{ExitKind, ShadowExecutor, InProcessExecutor}, stages::TracingStage, bolts::tuples::tuple_list};
// use libafl_targets::libfuzzer_test_one_input;



// pub type HARNESS = dyn Fn(&BytesInput) -> ExitKind;

// pub type LibfuzzerInprocessExecutor<'a, I, OT, S> = InProcessExecutor<'a, HARNESS, I, OT, S>;
// pub type LibfuzzerExecutor<'a, I, OT, S, SOT> = ShadowExecutor<LibfuzzerInprocessExecutor<'a, I, OT, S>, I, S, SOT>;

// pub fn get_libfuzzer_in_memory_tracing_stage<EM, I, OT, S, TE, Z>() -> TracingStage<EM, I, OT, S, LibfuzzerExecutor<'a, I, OT, S>, Z>{
//     let mut harness = |input: &BytesInput| {
//         let target = input.target_bytes();
//         let buf = target.as_slice();
//         libfuzzer_test_one_input(buf);
//         ExitKind::Ok
//     };
//     // Create the executor for an in-process function with just one observer for edge coverage
//     let mut executor = ShadowExecutor::new(
//         InProcessExecutor::new(
//             &mut harness,
//             tuple_list!(edges_observer, time_observer),
//             &mut fuzzer,
//             &mut state,
//             &mut restarting_mgr,
//         )?,
//         tuple_list!(cmplog_observer),
//     );
//     TracingStage::new(executor)
// }

use std::hash::{BuildHasher, Hash, Hasher};
use std::path::Path;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use once_cell::sync::Lazy;

use libafl::inputs::HasTargetBytes;
use libafl_bolts::prelude::AsSlice;
use itertools::Itertools;

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = ahash::RandomState::with_seeds(0, 0, 0, 0).build_hasher();
    hasher.write(bytes);
    let hash = hasher.finish();
    hash
}
pub fn hash_target_bytes_input<I: HasTargetBytes>(input: &I) -> u64 {
    let input_bytes = input.target_bytes();
    hash_bytes(&input_bytes.as_slice())
}
pub fn ensure_baseline_inputs_exist(dir: &Path) -> Result<(), std::io::Error> {
    log::info!("Creating baseline inputs in {:?}", dir);
    let baseline_dir = dir.join("symcts_baseline_inputs");
    std::fs::create_dir_all(&baseline_dir).unwrap();
    std::fs::write(
        baseline_dir.join("1024"),
        vec![69; 1024],
    ).unwrap();
    std::fs::write(
        baseline_dir.join("256"),
        vec![69; 256],
    ).unwrap();
    std::fs::write(
        baseline_dir.join("64"),
        vec![69; 64],
    ).unwrap();
    std::fs::write(
        baseline_dir.join("32"),
        vec![69; 32],
    ).unwrap();
    std::fs::write(
        baseline_dir.join("16"),
        vec![69; 16],
    ).unwrap();
    std::fs::write(
        baseline_dir.join("4"),
        vec![69; 64],
    ).unwrap();
    Ok(())
}


struct TimeTracker {
    times: HashMap<String, u128>,
    last_dumped: Instant,
    start_time: Instant,
}

// simple global timing map: auto-creates entries on first record
static COVERAGE_STAGE_TIMES: Lazy<Mutex<TimeTracker>> = Lazy::new(|| {
    Mutex::new(TimeTracker {
        times: HashMap::new(),
        last_dumped: Instant::now(),
        start_time: Instant::now(),
    })
});

pub fn dump_total_time_snapshot() {
    let tracker = COVERAGE_STAGE_TIMES.lock().unwrap();
    let snapshot_path = "/tmp/symcts_times_snapshot.txt";
    let mut snapshot_content = String::new();
    for (slot_name, total_ns) in tracker.times.iter().sorted() {
        snapshot_content.push_str(&format!("{}: {}\n", slot_name, total_ns))
    }
    snapshot_content.push_str(&format!("total_time: {}\n", tracker.start_time.elapsed().as_nanos()));
    std::fs::write(snapshot_path, &snapshot_content).unwrap();
    eprintln!("Final time snapshot written to {}:\n{}", snapshot_path, snapshot_content);
}
pub fn dump_percentage_time_snapshot() {
    let tracker = COVERAGE_STAGE_TIMES.lock().unwrap();
    let total_time_ns: u128 = tracker.times.values().sum();
    let snapshot_path = "/tmp/symcts_times_snapshot_percentages.txt";
    let mut snapshot_content = String::new();
    for (slot_name, total_ns) in tracker.times.iter().sorted() {
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
    let mut tracker = COVERAGE_STAGE_TIMES.lock().unwrap();
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

// A simple object that measures elapsed time and records it into a named slot on drop
pub struct TimeRecorder {
    name: String,
    start: Instant,
}
impl TimeRecorder {
    pub fn new(name: &str) -> Self {
        TimeRecorder {
            name: name.to_string(),
            start: Instant::now(),
        }
    }
    pub fn record_time(&self) {
        add_time_for_slot(&self.name, self.start.elapsed());
    }
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}
impl Drop for TimeRecorder {
    fn drop(&mut self) {
        self.record_time();
    }
}