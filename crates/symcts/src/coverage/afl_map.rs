use std::{marker::PhantomData, collections::HashSet};

use libafl_bolts::{impl_serdeany, prelude::Rand, tuples::MatchName, AsIter, AsSlice, HasLen, Named};
use libafl::{
    common::HasMetadata, events::{Event, EventWithStats}, feedbacks::Feedback, inputs::Input, observers::StdMapObserver, prelude::{alloc::borrow::Cow, stats::{AggregatorOps, UserStats, UserStatsValue}, EventFirer, HasTargetBytes, ObserversTuple, StateInitializer}, state::{BetterStateTrait, HasClientPerfMonitor, HasExecutions, HasRand}, Error
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::metadata::global::SyMCTSGlobalMetadata;

use super::{SyMCTSTestCaseAnnotationFeedback, CoverageSummary, loop_bucketing::get_bucketed_hitcount_outer, SingleCoverage};

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct AFLBitmapCoveragePoint {
    pub branch_index: usize,
    pub bucketed_count: usize,
    // pub reached_adjacent_edge: bool,
    // pub reached_function: bool,
}

impl AFLBitmapCoveragePoint {
    pub fn new(branch_index: usize, bitmap_entry: u32) -> Option<Self> {
        if bitmap_entry == 0 {
            return None;
        }
        let reached_adjacent_edge: bool = (bitmap_entry & (1 << 31)) != 0;
        let count = (bitmap_entry & 0x3FFFFFFF) as usize;
        if count == 0 && !reached_adjacent_edge {
            return None;
        }

        let bucketed_count = get_bucketed_hitcount_outer(count);

        return Some(AFLBitmapCoveragePoint {
            branch_index,
            bucketed_count,
        });
    }
}

impl core::fmt::Debug for AFLBitmapCoveragePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:x} * {}", self.branch_index, self.bucketed_count)
    }
}
impl core::fmt::Display for AFLBitmapCoveragePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as core::fmt::Debug>::fmt(&self, f)
    }
}


#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AFLBitmapCoverageMetadata {
    pub coverage_summary: CoverageSummary,
}
impl_serdeany!(AFLBitmapCoverageMetadata);

#[derive(Debug)]
pub struct SyMCTSAFLBitmapCoverageFeedback {
    name: Cow<'static, str>,
    map_observer_name: Cow<'static, str>,
    last_cov: Option<(CoverageSummary, SingleCoverage)>,
}

impl SyMCTSAFLBitmapCoverageFeedback {
    /// Creates a concolic feedback from an observer
    #[allow(unused)]
    #[must_use]
    pub fn for_afl_bitmap_observer<T>(observer: &StdMapObserver<T, false>) -> Self
    where
        T: Serialize + DeserializeOwned + Default + Copy,
    {
        Self {
            name: format!("SyMCTSFeedback_afl_{}", observer.name()).into(),
            map_observer_name: observer.name().clone().into(),
            last_cov: None,
        }
    }

    pub fn take_last_cov(&mut self) -> Option<(CoverageSummary, SingleCoverage)>{
        self.last_cov.take()
    }
}

impl Named for SyMCTSAFLBitmapCoverageFeedback {
    fn name(&self) -> &Cow<'static, str> {
        &self.name
    }
}


impl SyMCTSTestCaseAnnotationFeedback for SyMCTSAFLBitmapCoverageFeedback {
    fn get_coverage_points<I, S, OT: MatchName>(
        &self, input: &I, observers: &OT
    ) -> Result<(CoverageSummary, SingleCoverage), libafl::Error>
    where
        I: HasLen
    {
        let map_metadata = observers
            .match_name::<StdMapObserver<u32, false>>(&self.map_observer_name)
            .ok_or_else( || libafl::Error::illegal_state("Must have hitcounts map observer!"))?;

        // println!("Non-zero map entries: {:#?}", map_metadata.as_iter().enumerate().filter(|(idx, count)| **count != 0).map(|x| x.0).collect::<Vec<_>>());

        let coverage_points = map_metadata
            .as_iter()
            .enumerate()
            .filter_map(|(byte_idx, count)| AFLBitmapCoveragePoint::new(byte_idx, *count))
            .collect::<HashSet<AFLBitmapCoveragePoint>>();

        // log::debug!(target: "symcts_feedback", "Coverage points: {:?}", coverage_points.iter().sorted().collect::<Vec<_>>());

        let single_cov_map = SingleCoverage::from_shm_slice(input.len(), map_metadata.as_slice());
        Ok((CoverageSummary {
            points: coverage_points,
            trace_length: single_cov_map.non_zero_bitmap.count_ones(), // approximate trace length: the number of branches hit
            input_length: map_metadata.as_slice().len(),
        }, single_cov_map))
    }
}

impl<S> StateInitializer<S> for SyMCTSAFLBitmapCoverageFeedback
where
    S: HasMetadata,
{
    fn init_state(&mut self, state: &mut S) -> Result<(), Error> {
        state.add_metadata(SyMCTSGlobalMetadata::default());
        Ok(())
    }
}
impl<EM, I, OT, S> Feedback<EM, I, OT, S> for SyMCTSAFLBitmapCoverageFeedback
where
    EM: EventFirer<I, S>,
    S: HasClientPerfMonitor + HasMetadata + HasRand + BetterStateTrait<I> + HasExecutions,
    I: Input + HasLen + HasTargetBytes,
    OT: ObserversTuple<I, S>
{
    fn is_interesting(
        &mut self,
        state: &mut S,
        manager: &mut EM,
        input: &I,
        observers: &OT,
        exit_kind: &libafl::executors::ExitKind,
    ) -> Result<bool, libafl::Error>
    {
        log::debug!(target: "symcts_feedback", "Target reported exit kind of {:?}", exit_kind);
        let branches_before = { state.metadata::<SyMCTSGlobalMetadata>().unwrap().coverage_point_info.len() };

        let (cov_summary, single_cov) = self.get_coverage_points(input, observers)?;

        let (modified_global, _testcase_len) = self.record_metadata(
            state, input, observers,
            &cov_summary, &single_cov,
            exit_kind
        )?;
        let branches_after = { state.metadata::<SyMCTSGlobalMetadata>().unwrap().coverage_point_info.len() };
        assert!(branches_after >= branches_before);

        log::debug!(target: "symcts_feedback", "Coverage summary: {:?}", cov_summary);

        if modified_global || state.rand_mut().next() % 100 == 0 {
            manager.fire(
                state,
                EventWithStats::with_current_time(Event::UpdateUserStats {
                    name: "symcts_cov".into(),
                    // value: UserStats::Ratio(num_cov_points as u64, state.metadata::<SyMCTSGlobalMetadata>().unwrap().coverage_point_info.len() as u64),
                    value: UserStats::new(
                        UserStatsValue::Number(state.metadata::<SyMCTSGlobalMetadata>().unwrap().coverage_point_info.len() as u64),
                        AggregatorOps::None,
                    ),
                    phantom: PhantomData,
                },
                *state.executions()
            ))?;
        }

        let global_meta = state.metadata_mut::<SyMCTSGlobalMetadata>().unwrap();

        global_meta.total_num_times_traced += 1;
        global_meta.last_traced_cov = Some((cov_summary, single_cov, branches_before));

        if let Some((event, count)) = match exit_kind {
            libafl::executors::ExitKind::Crash => {
                global_meta.total_num_times_crashed += 1;
                Some(("crashes", global_meta.total_num_times_crashed))
            },
            libafl::executors::ExitKind::Timeout => {
                global_meta.total_num_times_timed_out += 1;
                Some(("timeouts", global_meta.total_num_times_timed_out))
            },
            _ => None
        } {
            manager.fire(
                state,
                EventWithStats::with_current_time(Event::UpdateUserStats {
                    name: event.into(),
                    // value: UserStats::Ratio(num_cov_points as u64, state.metadata::<SyMCTSGlobalMetadata>().unwrap().coverage_point_info.len() as u64),
                    value: UserStats::new(
                        UserStatsValue::Number(count as u64),
                        AggregatorOps::None,
                    ),
                    phantom: PhantomData,
                }, *state.executions())
            )?;
        }
        return Ok(modified_global);
    }
}
