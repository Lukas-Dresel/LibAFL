use std::{collections::HashSet, marker::PhantomData, time::Duration};

use libafl_bolts::{impl_serdeany, prelude::Rand, tuples::MatchName, AsIter, AsSlice, HasLen, Named};
use libafl::{
    common::HasMetadata,
    events::{Event},
    feedbacks::Feedback,
    inputs::Input,
    observers::StdMapObserver,
    prelude::{
        alloc::borrow::Cow,
        monitors::{AggregatorOps, UserStats, UserStatsValue},
        ExplicitTracking,
        EventFirer,
        HasTargetBytes,
        ObserversTuple,
        UsesInput,
    },
    state::{BetterStateTrait, MaybeHasClientPerfMonitor, HasExecutions, HasRand},
    Error
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{coverage::vectorized_coverage_map::CounterType, metadata::global::SyMCTSGlobalMetadata, util::{add_time_for_slot, TimeRecorder}};

use super::{SyMCTSTestCaseAnnotationFeedback, CoverageSummary, loop_bucketing::get_bucketed_hitcount_outer, SingleCoverage};

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct AFLBitmapCoveragePoint {
    pub branch_index: usize,
    pub bucketed_count: usize,
    // pub reached_adjacent_edge: bool,
    // pub reached_function: bool,
}

impl AFLBitmapCoveragePoint {
    pub fn new(branch_index: usize, bitmap_entry: CounterType) -> Option<Self> {
        if bitmap_entry == 0 {
            return None;
        }
        #[cfg(feature = "symcts_32bit_counters")]
        let (reached_adjacent_edge, count) = {
            let reached_adjacent_edge: bool = (bitmap_entry & (1 << 31)) != 0;
            let count = (bitmap_entry & 0x3FFFFFFF) as usize;
            (reached_adjacent_edge, count)
        };
        #[cfg(not(feature = "symcts_32bit_counters"))]
        let (reached_adjacent_edge, count) = (false, bitmap_entry as usize);

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
pub struct SyMCTSAFLBitmapCoverageFeedback<I> {
    name: Cow<'static, str>,
    map_observer_name: Cow<'static, str>,
    time_observer_name: Cow<'static, str>,
    last_cov: Option<(CoverageSummary, SingleCoverage)>,
    phantom: PhantomData<I>,
}

impl<I> SyMCTSAFLBitmapCoverageFeedback<I> {
    /// Creates a concolic feedback from an observer
    #[allow(unused)]
    #[must_use]
    pub fn for_afl_bitmap_observer<T>(observer: &StdMapObserver<T, false>, time_observer_name: impl Into<Cow<'static, str>>) -> Self
    where
        T: Serialize + DeserializeOwned + Default + Copy,
    {
        Self {
            name: format!("SyMCTSFeedback_afl_{}", observer.name()).into(),
            map_observer_name: observer.name().clone().into(),
            time_observer_name: time_observer_name.into(),
            last_cov: None,
            phantom: PhantomData,
        }
    }
    #[allow(unused)]
    #[must_use]
    pub fn for_tracking_afl_bitmap_observer<T>(observer: &ExplicitTracking<StdMapObserver<T, false>, true, false>, time_observer_name: impl Into<Cow<'static, str>>) -> Self
    where
        T: Serialize + DeserializeOwned + Default + Copy,
    {
        Self {
            name: format!("SyMCTSFeedback_afl_{}", observer.name()).into(),
            map_observer_name: observer.name().clone().into(),
            time_observer_name: time_observer_name.into(),
            last_cov: None,
            phantom: PhantomData,
        }
    }

    pub fn take_last_cov(&mut self) -> Option<(CoverageSummary, SingleCoverage)>{
        self.last_cov.take()
    }
}

impl<I> Named for SyMCTSAFLBitmapCoverageFeedback<I> {
    fn name(&self) -> &Cow<'static, str> {
        &self.name
    }
}


impl<I> SyMCTSTestCaseAnnotationFeedback for SyMCTSAFLBitmapCoverageFeedback<I> {
    fn get_coverage_points<I2, S, OT: MatchName>(
        &self, input: &I2, observers: &OT
    ) -> Result<(CoverageSummary, SingleCoverage), libafl::Error>
    where
        I2: HasLen
    {
        let tr_full = TimeRecorder::new("get_coverage_points");
        let map_metadata = observers
            .match_name::<StdMapObserver<CounterType, false>>(&self.map_observer_name)
            .ok_or_else( || libafl::Error::illegal_state(format!("Must have hitcounts map observer! Expected to find {} of type {}, got {}",
                &self.map_observer_name,
                std::any::type_name::<StdMapObserver<CounterType, false>>(),
                std::any::type_name::<OT>(),
            )))?;

        let tr_get_points_iter = TimeRecorder::new("get_coverage_points::getting_map_iter");
        let coverage_points = map_metadata
            .as_iter()
            .enumerate()
            .filter_map(|(byte_idx, count)| AFLBitmapCoveragePoint::new(byte_idx, *count))
            .collect::<HashSet<AFLBitmapCoveragePoint>>();
        drop(tr_get_points_iter); // log time

        // log::debug!(target: "symcts_feedback", "Coverage points: {:?}", coverage_points.iter().sorted().collect::<Vec<_>>());

        let tr_from_shm_slice = TimeRecorder::new("get_coverage_points::from_shm_slice");
        let single_cov_map = SingleCoverage::from_shm_slice(input.len(), map_metadata.as_slice());
        drop(tr_from_shm_slice); // log time

        Ok((CoverageSummary {
            points: coverage_points,
            trace_length: single_cov_map.non_zero_bitmap.count_ones(), // approximate trace length: the number of branches hit
            input_length: map_metadata.as_slice().len(),
        }, single_cov_map))
    }
    
}

// impl<S> StateInitializer<S> for SyMCTSAFLBitmapCoverageFeedback
// where
//     S: HasMetadata,
// {
    
// }
impl<S, I> Feedback<S> for SyMCTSAFLBitmapCoverageFeedback<I>
where
    // EM: EventFirer,
    I: Input + HasLen + HasTargetBytes,
    // OT: ObserversTuple<S>,
    S: MaybeHasClientPerfMonitor + HasMetadata + HasRand + BetterStateTrait<I> + HasExecutions,
{
    fn init_state(&mut self, state: &mut S) -> Result<(), Error> {
        state.add_metadata(SyMCTSGlobalMetadata::default());
        Ok(())
    }
    fn is_interesting<EM: EventFirer<State=S>, OT: MatchName + ObserversTuple<S>>(
        &mut self,
        state: &mut S,
        manager: &mut EM,
        input: &S::Input,
        observers: &OT,
        exit_kind: &libafl::executors::ExitKind,
    ) -> Result<bool, libafl::Error>
    {
        let tr_full = TimeRecorder::new("symcts_feedback_is_interesting");
        log::debug!(target: "symcts_feedback", "Target reported exit kind of {:?}", exit_kind);
        let branches_before = { state.metadata::<SyMCTSGlobalMetadata>().unwrap().num_covered_branches() };

        let tr_is_interesting_get_coverage_points: TimeRecorder = TimeRecorder::new("symcts_feedback_is_interesting::get_coverage_points");
        let (cov_summary, single_cov) = self.get_coverage_points(input, observers)?;
        drop(tr_is_interesting_get_coverage_points); // log time
        
        let time_observer = observers
            .match_name::<libafl::observers::TimeObserver>(&self.time_observer_name)
            .expect("Failed to get TimeObserver by name");
        let exec_time_millis = time_observer.last_runtime().unwrap().as_millis() as usize;
        add_time_for_slot("target_execution", Duration::from_millis(exec_time_millis as u64));

        let tr_symcts_afl_feedback_record_metadata = TimeRecorder::new("symcts_feedback_is_interesting::record_metadata");
        let (modified_global, _testcase_len) = self.record_metadata(
            state, input, observers,
            &cov_summary, &single_cov,
            exec_time_millis,
            exit_kind
        )?;
        drop(tr_symcts_afl_feedback_record_metadata); // log time

        let branches_after = { state.metadata::<SyMCTSGlobalMetadata>().unwrap().num_covered_branches() };
        assert!(branches_after >= branches_before);

        log::debug!(target: "symcts_feedback", "Coverage summary: {:?}", cov_summary);

        if modified_global || state.rand_mut().next() % 100 == 0 {
            manager.fire(
                state,
                Event::UpdateUserStats {
                    name: "symcts_cov".into(),
                    // value: UserStats::Ratio(num_cov_points as u64, state.metadata::<SyMCTSGlobalMetadata>().unwrap().num_covered_branches() as u64),
                    value: UserStats::new(
                        UserStatsValue::Number(state.metadata::<SyMCTSGlobalMetadata>().unwrap().num_covered_branches() as u64),
                        AggregatorOps::None,
                    ),
                    phantom: PhantomData,
                }
            )?;
        }

        let tr_postprocessing = TimeRecorder::new("symcts_feedback_is_interesting::postprocessing");
        let global_meta = state.metadata_mut::<SyMCTSGlobalMetadata>().unwrap();

        global_meta.total_num_times_traced += 1;
        global_meta.total_time_spent_tracing_millis += exec_time_millis;
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
                Event::UpdateUserStats {
                    name: event.into(),
                    // value: UserStats::Ratio(num_cov_points as u64, state.metadata::<SyMCTSGlobalMetadata>().unwrap().coverage_point_info.len() as u64),
                    value: UserStats::new(
                        UserStatsValue::Number(count as u64),
                        AggregatorOps::None,
                    ),
                    phantom: PhantomData,
                }
            )?;
        }
        return Ok(modified_global);
    }
}
