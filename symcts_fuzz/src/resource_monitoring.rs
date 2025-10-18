//! The [`SyncFromAFLStage`] is a stage that imports inputs from disk for e.g. sync with AFL
use std::{
    fs,
    time::SystemTime,
};

use libafl_bolts::impl_serdeany;
use serde::{Deserialize, Serialize};
use std::io::Write;

use libafl::{
    corpus::CorpusId,
};

use crate::metadata::global::SyMCTSGlobalMetadata;

#[cfg(feature = "resource_tracking_per_branch")]
use crate::coverage::CoveragePoint;

use std::collections::HashMap;

// pub struct ResourceInfo {
//     pub testcases_disk_space_usage_total: u64,
//     pub num_testcases_total: u64,
//     pub 
// ;}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BranchResourceUsageMetadata {
    pub num_testcases: usize,
    pub testcase_disk_space_usage: usize,

    // pub num_always_colocated_branches: usize,
    // pub num_never_colocated_branches: usize,
    // pub num_sometimes_colocated_branches: usize,
}

/// Metadata used to store information about disk sync time
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResourceUsageMetadata {
    /// The last time the sync was done
    pub time: SystemTime,
    pub current_tick: usize,

    pub last_tick_seen_new_branch: usize,

    pub coverage_points_seen: usize,

    pub time_since_last_sync: Option<u64>,

    pub testcases_disk_space_usage_total: usize,
    pub num_testcases_total: usize,

    pub ram_usage_current: usize,
    pub ram_usage_max: usize,

    pub per_branch_info: HashMap<String, BranchResourceUsageMetadata>,
}   

impl_serdeany!(ResourceUsageMetadata);

impl Default for ResourceUsageMetadata {
    fn default() -> Self {
        Self {
            time: SystemTime::now(),
            current_tick: 0,
            last_tick_seen_new_branch: 0,
            coverage_points_seen: 0,
            time_since_last_sync: None,
            testcases_disk_space_usage_total: 0,
            num_testcases_total: 0,
            ram_usage_current: 0,
            ram_usage_max: 0,
            per_branch_info: HashMap::new(),
        }
    }
}


pub struct StatMResults {
    pub size: u64,      // total program size
    pub resident: u64,  // resident set size
    pub share: u64,     // shared pages (from shared mappings) (i.e., backed by a file)
    pub text: u64,      // text (code)
    pub lib: u64,       // library (unused since 2.6; always 0)
    pub data: u64,      // data + stack
    pub dt: u64,        // dirty pages
}
impl StatMResults {
    pub fn new(size: u64, resident: u64, share: u64, text: u64, lib: u64, data: u64, dt: u64) -> Self {
        Self {
            size,
            resident,
            share,
            text,
            lib,
            data,
            dt,
        }
    }

    pub fn for_self() -> Self {
        let status = fs::read_to_string("/proc/self/statm").unwrap();
        let parts: Vec<&str> = status.split_whitespace().collect();
        let size = parts[0].parse::<u64>().unwrap();
        let resident = parts[1].parse::<u64>().unwrap();
        let share = parts[2].parse::<u64>().unwrap();
        let text = parts[3].parse::<u64>().unwrap();
        let lib = parts[4].parse::<u64>().unwrap();
        let data = parts[5].parse::<u64>().unwrap();
        let dt = parts[6].parse::<u64>().unwrap();
        Self::new(size, resident, share, text, lib, data, dt)
    }

    pub fn for_pid(pid: u32) -> Self {
        let status = fs::read_to_string(format!("/proc/{}/statm", pid)).unwrap();
        let parts: Vec<&str> = status.split_whitespace().collect();
        let size = parts[0].parse::<u64>().unwrap();
        let resident = parts[1].parse::<u64>().unwrap();
        let share = parts[2].parse::<u64>().unwrap();
        let text = parts[3].parse::<u64>().unwrap();
        let lib = parts[4].parse::<u64>().unwrap();
        let data = parts[5].parse::<u64>().unwrap();
        let dt = parts[6].parse::<u64>().unwrap();
        Self::new(size, resident, share, text, lib, data, dt)
    }
}
fn get_current_memory_usage() -> usize {
    // parse /proc/self/status
    let statm = StatMResults::for_self();
    (statm.resident * 4096) as usize
}

pub fn update_resource_tracker_on_scheduling(
    global_meta: &mut SyMCTSGlobalMetadata,
    last_synced_time: Option<SystemTime>,
    _corpus_idx: CorpusId,
)
{
    let metadata_path = {
        global_meta.sync_dir.join(".resource_monitoring_metadata.jsonl")
    };

    let current_tick = global_meta.current_tick();
    let last_tick_seen_new_branch = global_meta.last_tick_seen_new_branch;
    let coverage_points_seen = global_meta.num_covered_branches();

    let resources = &mut global_meta.tracked_resources;

    resources.time = SystemTime::now();
    resources.current_tick = current_tick;
    resources.last_tick_seen_new_branch = last_tick_seen_new_branch;
    resources.coverage_points_seen = coverage_points_seen;

    resources.ram_usage_current = get_current_memory_usage();
    resources.ram_usage_max = resources.ram_usage_max.max(resources.ram_usage_current);
    resources.time_since_last_sync = last_synced_time.map(|l| SystemTime::now().duration_since(l).unwrap().as_secs());

    // now serialize the metadata to disk
    let metadata_str = serde_json::to_string(&resources).unwrap();

    // append to the file
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(metadata_path)
        .unwrap();
    file.write_all(metadata_str.as_bytes()).unwrap();
    file.write_all(b"\n").unwrap();
}


pub fn update_resource_tracker_on_new_corpus_entry(
    global_meta: &mut SyMCTSGlobalMetadata,
    input_size: usize,
)
{
    let resources = &mut global_meta.tracked_resources;
    resources.num_testcases_total += 1;
    resources.testcases_disk_space_usage_total += input_size;
}

#[cfg(feature = "resource_tracking_per_branch")]
pub fn update_resource_tracker_on_branch_corpus_addition(
    global_meta: &mut SyMCTSGlobalMetadata,
    corpus_idx: CorpusId,
    input_size: usize,
    coverage_point: &CoveragePoint,
)
{
    let resources = &mut global_meta.tracked_resources;
    let branch_resources = resources.per_branch_info.entry(format!("{:?}", coverage_point)).or_insert_with(|| BranchResourceUsageMetadata {
        num_testcases: 0,
        testcase_disk_space_usage: 0,
    });

    branch_resources.num_testcases += 1;
    branch_resources.testcase_disk_space_usage += input_size;
}
