//! Map feedback, maximizing or minimizing maps, for example the afl-style map observer.
use crate::coverage::CoverageMinMaxTracker;
use crate::coverage::{CoveragePoint, CoverageSummary, SingleCoverage};
use crate::symcts_mutations::{MutationSource, MutationResultMetadata};
 #[cfg(feature = "coverage_single_level")]
use crate::coverage::vectorized_coverage_map::VectorizedCoverage;

use bitvec::vec::BitVec;

#[cfg(feature="resource_tracking")]
use crate::resource_monitoring::{
    ResourceUsageMetadata,
    update_resource_tracker_on_scheduling,
    update_resource_tracker_on_new_corpus_entry
};
#[cfg(feature = "resource_tracking_per_branch")]
use crate::resource_monitoring::update_resource_tracker_on_branch_corpus_addition;

use core::fmt::Debug;
use std::path::PathBuf;
use libafl_bolts::HasLen;
use libafl::prelude::{Input};
use libafl::corpus::{Corpus, CorpusId};
use libafl::common::HasMetadata;
use libafl::state::{HasCorpus, HasRand, BetterStateTrait};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone)]
pub struct CoverageLocationInfo {
    pub num_times_symbolically_sampled: usize,
    pub num_times_coverage_traced: usize,
    pub time_spent_sampling_millis: usize,
    pub time_spent_tracing_millis: usize,
    pub tick_last_seen_mutated: usize,
    pub coverage_min_max_tracker: Option<CoverageMinMaxTracker>,
}
libafl_bolts::impl_serdeany!(CoverageLocationInfo);

impl core::fmt::Debug for CoverageLocationInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f
            .debug_struct("CoverageLocationInfo")
                .field("min_max_coverage", &self.coverage_min_max_tracker)
                .field("num_times_symbolically_sampled", &self.num_times_symbolically_sampled)
                .field("num_times_coverage_traced", &self.num_times_coverage_traced).finish()
    }
}

impl CoverageLocationInfo {
    pub fn filtered_covering_corpus_ids(&self, predicate: impl Fn(&CorpusId) -> bool) -> Vec<CorpusId> {
        self.coverage_min_max_tracker.as_ref().unwrap().filtered_covering_corpus_ids(predicate)
    }
    pub fn update_testcase_for_newly_triggered_coverage_points(
        &mut self,
        _cov_point: &CoveragePoint,
        corpus_id: CorpusId,
        cov: &SingleCoverage,
    ) {
        match self.coverage_min_max_tracker {
            Some(ref mut tracker) => {
                #[cfg(not(feature = "coverage_single_level"))]
                tracker.add(&cov, corpus_id);
                #[cfg(feature = "coverage_single_level")]
                tracker.add(
                    &VectorizedCoverage::from_element(
                        cov.input_length_exponent,
                        cov.count_for_branch(_cov_point.branch_index)
                    ),
                    corpus_id);
            }
            None => {
                #[cfg(not(feature = "coverage_single_level"))]
                {
                    self.coverage_min_max_tracker = Some(
                        CoverageMinMaxTracker::create(cov, corpus_id)
                    );
                }

                #[cfg(feature = "coverage_single_level")]
                {
                    let temp_cov = VectorizedCoverage::from_element(
                        cov.input_length_exponent,
                        cov.count_for_branch(cov_point.branch_index)
                    );
                    self.coverage_min_max_tracker = Some(
                        CoverageMinMaxTracker::create(
                            &temp_cov,
                            corpus_id
                        )
                    );
                }
            }
        }
    }
}

// TODO maybe use bignums instead of just usize, for now use `checked_add` to see if necessary
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyMCTSGlobalMetadata {
    pub sync_dir: PathBuf,
    // Indexed by branch_index, with BitVec for fast iteration over covered branches
    pub coverage_point_info: Vec<Option<CoverageLocationInfo>>,
    pub coverage_present_bitmap: BitVec,
    pub total_num_times_sampled: usize,
    pub total_num_times_traced: usize,
    pub total_time_spent_sampling_millis: usize,
    pub total_time_spent_tracing_millis: usize,
    pub total_num_times_crashed: usize,
    pub total_num_times_timed_out: usize,
    pub synced_inputs_queue: Vec<CorpusId>,
    pub last_tick_seen_new_branch: usize,
    pub last_scheduled: Option<(CoveragePoint, CorpusId)>,
    pub current_mutation_source: Option<MutationSource>,
    pub last_traced_cov: Option<(CoverageSummary, SingleCoverage, usize)>, // the last usize is the coverage_point_info count before the input was traced
    pub hash_to_corpus_id: HashMap<u64, CorpusId>,

    #[cfg(feature="resource_tracking")]
    pub tracked_resources: ResourceUsageMetadata,
}
libafl_bolts::impl_serdeany!(SyMCTSGlobalMetadata);

impl Default for SyMCTSGlobalMetadata {
    fn default() -> Self {
        Self {
            sync_dir: PathBuf::default(),
            coverage_point_info: Vec::new(),
            coverage_present_bitmap: BitVec::new(),
            total_num_times_sampled: 0,
            total_num_times_traced: 0,
            total_time_spent_sampling_millis: 0,
            total_time_spent_tracing_millis: 0,
            total_num_times_crashed: 0,
            total_num_times_timed_out: 0,
            synced_inputs_queue: Vec::new(),
            last_tick_seen_new_branch: 0,
            last_scheduled: None,
            current_mutation_source: None,
            last_traced_cov: None,
            hash_to_corpus_id: HashMap::new(),
            #[cfg(feature="resource_tracking")]
            tracked_resources: ResourceUsageMetadata::default(),
        }
    }
}

impl SyMCTSGlobalMetadata {
    pub fn current_tick(&self) -> usize {
        self.total_num_times_sampled
    }
    pub fn increment_tick(&mut self) {
        self.total_num_times_sampled += 1;
    }
    pub fn seems_stuck(&self) -> bool {
        let seems_stuck = self.last_tick_seen_new_branch + 10 < self.current_tick();
        log::info!(
            "last_tick_seen_new_branch: {}, current_tick: {} => stuck: {}",
            self.last_tick_seen_new_branch,
            self.current_tick(),
            seems_stuck
        );
        seems_stuck
    }
    pub fn reset_stuck_counter(&mut self) {
        self.last_tick_seen_new_branch = self.current_tick();
    }

    /// Get coverage info for a branch index, returns None if not covered
    #[inline(always)]
    pub fn get_coverage_info(&self, branch_index: usize) -> Option<&CoverageLocationInfo> {
        self.coverage_point_info.get(branch_index)?.as_ref()
    }

    /// Get mutable coverage info for a branch index, returns None if not covered
    #[inline(always)]
    pub fn get_coverage_info_mut(&mut self, branch_index: usize) -> Option<&mut CoverageLocationInfo> {
        self.coverage_point_info.get_mut(branch_index)?.as_mut()
    }

    /// Ensure space exists for branch_index and return mutable reference, creating if necessary
    pub fn get_or_insert_coverage_info(&mut self, branch_index: usize) -> &mut CoverageLocationInfo {
        // Ensure Vec is large enough
        if branch_index >= self.coverage_point_info.len() {
            self.coverage_point_info.resize(branch_index + 1, None);
        }
        // Ensure BitVec is large enough
        if branch_index >= self.coverage_present_bitmap.len() {
            self.coverage_present_bitmap.resize(branch_index + 1, false);
        }

        // Get or create the entry
        let entry = &mut self.coverage_point_info[branch_index];
        if entry.is_none() {
            *entry = Some(CoverageLocationInfo {
                num_times_symbolically_sampled: 0,
                num_times_coverage_traced: 0,
                time_spent_sampling_millis: 0,
                time_spent_tracing_millis: 0,
                tick_last_seen_mutated: 0,
                coverage_min_max_tracker: None,
            });
            self.coverage_present_bitmap.set(branch_index, true);
        }

        entry.as_mut().unwrap()
    }

    /// Count of covered branches
    pub fn num_covered_branches(&self) -> usize {
        self.coverage_present_bitmap.count_ones()
    }

    /// Iterate over all covered branch indices
    pub fn covered_branch_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.coverage_present_bitmap.iter_ones()
    }
}

// impl Serialize for SyMCTSGlobalMetadata {
//     fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: serde::Serializer {
//             let mut map = serializer.serialize_map(Some(self.coverage_point_info.len()))?;
//             for (k, v) in &self.coverage_point_info {
//                 map.serialize_entry(&format!("{:?}", k), v)?;
//             }
//             map.end()
//     }
// }

pub fn register_symbolic_sampling_of_testcase<I, S>(state: &mut S, sampled_id: CorpusId, input_len: usize, time_spent_millis: usize)
where
    I: Input + HasLen,
    S: HasMetadata + BetterStateTrait<I>,
{
    state
        .corpus_mut()
        .get(sampled_id)
        .expect("corpus id should exist to be scheduled for sampling")
        .borrow_mut()
        .metadata_mut::<MutationResultMetadata>()
        .unwrap().num_times_mutated += 1;

    let (_rng, corpus, meta) = state.get_state_components_rand_corpus_metadata();
    let testcase = corpus.get(sampled_id).unwrap().borrow();

    let cov_summary = testcase.metadata::<CoverageSummary>().unwrap().clone();
    
    let last_synced_time = None;

    let global_meta = meta
        .get_mut::<SyMCTSGlobalMetadata>()
        .unwrap();

    global_meta.increment_tick();
    for cov_point in cov_summary.points.iter() {
        let v = global_meta
            .get_coverage_info_mut(cov_point.branch_index)
            .unwrap_or_else(|| {
                panic!(
                    "Coverage point {:?} from {:?} not found in global metadata when trying to register symbolic sampling, should always have been added when it was first traced instead!", cov_point, sampled_id);
            });
        v.num_times_symbolically_sampled += 1;
    }

    #[cfg(feature="resource_tracking")]
    update_resource_tracker_on_scheduling(global_meta, last_synced_time, sampled_id);
}

pub fn register_new_interesting_inputs<I, S>(state: &mut S, traced_inputs: Vec<(CorpusId, CoverageSummary, SingleCoverage, usize, usize)>)
where
    S: HasCorpus<Input=I> + HasMetadata + HasRand + BetterStateTrait<I>,
    I: HasLen + Input
{
    let (_, corpus_ref, state_metadata) = state.get_state_components_rand_corpus_metadata();

    let global_meta = state_metadata.get_mut::<SyMCTSGlobalMetadata>().unwrap();

    for (corpus_id, cov_summary, cov_map, input_len, exec_time_millis) in traced_inputs.into_iter() {
        let mut testcase = corpus_ref.get(corpus_id).unwrap().borrow_mut();
        let input_size = testcase.load_input(corpus_ref).expect("could not load testcase input").len();

        // println!("Traced input: {:?}, tc_meta: {:?}", testcase.input(), &tc_meta);
        for cov_point in cov_summary.points.iter() {
            let branch_index = cov_point.branch_index;

            // Check if this is a new branch
            let is_new = global_meta.get_coverage_info(branch_index).is_none();

            let cov_info = if is_new {
                // Create new entry for this branch
                let info = global_meta.get_or_insert_coverage_info(branch_index);
                info.num_times_coverage_traced = 1;
                info.time_spent_tracing_millis = exec_time_millis;

                #[cfg(not(feature = "coverage_single_level"))]
                {
                    info.coverage_min_max_tracker = Some(CoverageMinMaxTracker::create(
                        &cov_map,
                        corpus_id
                    ));
                }
                #[cfg(feature = "coverage_single_level")]
                {
                    info.coverage_min_max_tracker = Some(
                        CoverageMinMaxTracker::create(
                            &VectorizedCoverage::from_element(
                                cov_map.input_length_exponent,
                                cov_map.count_for_branch(cov_point.branch_index)
                            ),
                            corpus_id
                        )
                    );
                }
                info
            } else {
                global_meta.get_coverage_info_mut(branch_index).unwrap()
            };

            cov_info.update_testcase_for_newly_triggered_coverage_points(&cov_point, corpus_id, &cov_map);
            #[cfg(all(feature="resource_tracking", feature="resource_tracking_per_branch"))]
            update_resource_tracker_on_branch_corpus_addition(global_meta, corpus_id, input_size, cov_point);
        }
        testcase.add_metadata(cov_summary);
        #[cfg(feature="resource_tracking")]
        update_resource_tracker_on_new_corpus_entry(global_meta, input_size);
    }



}
