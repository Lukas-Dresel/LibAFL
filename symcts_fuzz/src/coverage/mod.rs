pub mod afl_map;

pub mod loop_bucketing;
#[cfg(feature="coverage_vectorized")]
pub mod vectorized_coverage_map;
#[cfg(feature="coverage_vectorized")]
pub mod coverage_min_max_tracker_vectorized;
#[cfg(not(feature="coverage_vectorized"))]
pub mod non_vectorized_coverage_map;
#[cfg(not(feature="coverage_vectorized"))]
pub mod coverage_min_max_tracker_non_vectorized;

use std::collections::HashSet;
use std::io::Write;

use core::fmt::Formatter;

pub use afl_map::AFLBitmapCoveragePoint as CoveragePoint;
pub use afl_map::SyMCTSAFLBitmapCoverageFeedback as SyMCTSCoverageFeedback;

use libafl_bolts::impl_serdeany;
use libafl_bolts::{HasLen};
use libafl::executors::ExitKind;
use libafl::observers::ObserversTuple;
use libafl::state::BetterStateTrait;
use libafl::state::MaybeHasClientPerfMonitor;
use libafl::common::HasMetadata;
use libafl::state::HasRand;
use serde::Deserialize;
use serde::Serialize;

use crate::metadata::global::CoverageLocationInfo;
use crate::metadata::global::SyMCTSGlobalMetadata;
use crate::util::TimeRecorder;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum InterestReason {
    Minimizes { index: usize, old: usize, new: usize },
    Maximizes { index: usize, old: usize, new: usize },
    Novel,
    Longest { old: usize, new: usize },
    Shortest { old: usize, new: usize },
    Fastest { old: usize, new: usize },
    Slowest { old: usize, new: usize },
}

#[cfg(feature="coverage_vectorized")]
pub use self::vectorized_coverage_map::VectorizedCoverage as SingleCoverage;
#[cfg(feature="coverage_vectorized")]
pub use self::coverage_min_max_tracker_vectorized::CoverageMinMaxTracker as CoverageMinMaxTracker;

#[cfg(not(feature="coverage_vectorized"))]
pub use self::non_vectorized_coverage_map::NonVectorizedCoverage as SingleCoverage;
#[cfg(not(feature="coverage_vectorized"))]
pub use self::coverage_min_max_tracker_non_vectorized::CoverageMinMaxTracker as CoverageMinMaxTracker;


#[derive(Serialize, Deserialize, Default, Clone)]
pub struct CoverageSummary {
    // pub map: SingleCoverage,
    pub points: HashSet<CoveragePoint>,
    pub trace_length: usize,
    pub input_length: usize,
}
impl_serdeany!(CoverageSummary);

// implement the Debug trait for CoverageSummary
impl std::fmt::Debug for CoverageSummary {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut sorted_cov_points = self.points.iter().collect::<Vec<_>>();
        sorted_cov_points.sort();
        write!(f, "CoverageSummary {{ points: {:?}, trace_length: {}, input_length: {} }}", sorted_cov_points, self.trace_length, self.input_length)
    }
}

pub trait SyMCTSTestCaseAnnotationFeedback
{
    fn get_coverage_points<I, S, OT>(
        &self,
        input: &I,
        observers: &OT
    ) -> Result<(CoverageSummary, SingleCoverage), libafl::Error>
    where
          I: HasLen
        , S: BetterStateTrait<I>
        , OT: ObserversTuple<S>
        ;

    fn record_metadata<I, S, OT>(
        &self,
        state: &mut S,
        input: &I,
        _observers: &OT,
        coverage_summary: &CoverageSummary,
        cur_cov: &SingleCoverage,
        exec_time_millis: usize,
        exit_kind: &libafl::executors::ExitKind,
    ) -> Result<(bool, usize), libafl::Error>
    where
        I: HasLen,
        OT: ObserversTuple<S>,
        S: MaybeHasClientPerfMonitor + HasMetadata + HasRand + BetterStateTrait<I>,
    {
        let tr_full = TimeRecorder::new("symcts_feedback_record_metadata");
        let mut modified = false;

        match exit_kind {
            ExitKind::Timeout => {
                return Ok((false, 0));
            },
            ExitKind::Oom => {
                return Ok((false, 0));
            },
            ExitKind::Crash => {
                modified = true; // always consider crashes interesting
            },
            ExitKind::Diff { .. } => {
                modified = true; // always consider diffs interesting
            },
            ExitKind::Ok => {
                // do nothing
            },
        }
        let testcase_len = input.len();

        let (_, _corpus, metadata) = state.get_state_components_rand_corpus_metadata();
        let global_state = metadata.get_mut::<SyMCTSGlobalMetadata>().unwrap();

        let mut found = None;
        let time_before_interestingness_check = std::time::Instant::now();
        let tr_interesting = TimeRecorder::new("symcts_feedback_record_metadata--check_interestingness");
        for cov_point in &coverage_summary.points {
            let cov_info = global_state.coverage_point_info.entry(cov_point.clone()).or_insert_with(|| {
                let cov_info = CoverageLocationInfo {
                    coverage_min_max_tracker: None,
                    num_times_coverage_traced: 0,
                    num_times_symbolically_sampled: 0,
                    time_spent_sampling_millis: 0,
                    time_spent_tracing_millis: 0,
                    tick_last_seen_mutated: 0,
                };
                cov_info
            });

            cov_info.time_spent_tracing_millis += exec_time_millis;
            cov_info.num_times_coverage_traced += 1;

            #[cfg(not(feature = "coverage_single_level"))]
            let reason = if let Some(tracker) = &cov_info.coverage_min_max_tracker {
                use crate::util::TimeRecorder;

                let _tr_is_interesting_internal = TimeRecorder::new("symcts_feedback_record_metadata--is_interesting_internal"); // records drop on exit from frame
                tracker.is_interesting_for(&cur_cov)
            } else {
                Some(InterestReason::Novel)
            };

            #[cfg(feature = "coverage_single_level")]
            let reason = if let Some(tracker) = &cov_info.coverage_min_max_tracker {
                let tr_is_interesting_internal = TimeRecorder::new("symcts_feedback_record_metadata--is_interesting_internal");
                let tmp_cov = SingleCoverage::from_element(
                    cur_cov.input_length_exponent,
                    cur_cov.count_for_branch(cov_point.branch_index)
                );
                tracker.is_interesting_for(&tmp_cov)
            } else {
                Some(InterestReason::Novel)
            };
            if let Some(reason) = reason {
                found = Some((cov_point.clone(), reason));
                break;
            }
        }
        drop(tr_interesting);
        
        if let Some((cov_point, reason)) = found {
            let feedback_log_path = global_state.sync_dir.join(".feedback.log");
            let mut feedback_log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(feedback_log_path)
                .unwrap();
            let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
            let msg = format!("{}\t{:?}\t{:x?}\n", timestamp, cov_point, reason);
            feedback_log.write_all(msg.as_bytes()).unwrap();
            log::info!(target: "symcts_feedback", "New testcase for {:?} => {:x?} exec_time_millis={}", cov_point, reason, exec_time_millis);
        }
        Ok((modified || found.is_some(), testcase_len))

    }
}
