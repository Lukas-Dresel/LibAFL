use std::cmp::min;
use std::io::Write;
use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};

use itertools::{Itertools, MinMaxResult};
#[cfg(feature="scheduling_weight_function_time_spent")]
use libafl::prelude::weighted;
use libafl_bolts::HasLen;
use libafl::corpus::{Corpus, CorpusId};
use libafl::prelude::{HasTargetBytes, HasTestcase, LenTimeMulTestcaseScore, TestcaseScore, Input, State, HasCorpus, UsesInput, UsesState};
use libafl::schedulers::Scheduler;
use libafl::common::{HasMetadata};
use libafl::state::{HasRand, BetterStateTrait};
use libafl::Error;
use rand::prelude::SliceRandom;

use crate::coverage::{CoveragePoint, CoverageSummary};
use crate::metadata::global::{SyMCTSGlobalMetadata, CoverageLocationInfo, register_new_interesting_inputs, register_symbolic_sampling_of_testcase};
use crate::symcts_mutations::MutationResultMetadata;
use crate::util::{hash_target_bytes_input, TimeRecorder};


#[derive(Debug, Default, Clone, Copy)]
pub struct SyMCTSScheduler<S, SC> {
    inner_scheduler: SC,
    phantom: PhantomData<S>,
}

impl<S, SC> SyMCTSScheduler<S, SC> {
    pub fn new(inner_scheduler: SC) -> Self {
        Self {
            inner_scheduler,
            phantom: PhantomData,
        }
    }
}

fn num_times_mutated<I: Input, C: Corpus<Input=I>>(corpus: &C, id: CorpusId) -> usize {
    corpus
        .get(id)
        .unwrap()
        .borrow()
        .metadata::<MutationResultMetadata>()
        .unwrap()
        .num_times_mutated
}
fn trace_len<I: Input, C: Corpus<Input=I>>(corpus: &C, id: CorpusId) -> usize {
    corpus
        .get(id)
        .unwrap()
        .borrow()
        .metadata::<CoverageSummary>()
        .unwrap()
        .trace_length
}
fn input_length_and_exec_time<I, C>(corpus: &C, id: CorpusId) -> Result<(usize, usize), Error>
where
    C: Corpus<Input=I>,
    I: HasLen + HasTargetBytes + Input,
{
    let mut testcase = corpus.get(id).unwrap().borrow_mut();
    let exec_time_millis = testcase.exec_time().map_or(1, |d| d.as_millis() as usize);
    let input_len = testcase.load_len(corpus)?;
    Ok((input_len, exec_time_millis))
}

/// compute the len_time_mul score, this favors small quick inputs
pub fn len_time_mul_score(exec_time_millis: usize, len: usize) -> f64 {
    (len as f64) * (exec_time_millis as f64)
}

fn input_len<I, C: Corpus<Input=I>>(corpus: &C, id: CorpusId) -> usize
where
    I: HasLen + Input,
{
    corpus
        .get(id)
        .unwrap()
        .borrow()
        .input()
        .as_ref()
        .unwrap()
        .len()
}

impl<S: State, SC> UsesState for SyMCTSScheduler<S, SC> {
    type State = S;
}

impl<I, S, SC> Scheduler for SyMCTSScheduler<S, SC>
where
    S: HasRand + HasMetadata + HasTestcase<Input=I> + BetterStateTrait<I> + State,
    I: HasLen + HasTargetBytes + Input,
    SC: Scheduler<State=S>
{
    fn next(&mut self, state: &mut S) -> Result<CorpusId, Error> {
        let tr_total = TimeRecorder::new("SyMCTSScheduler::next");

        {
            let tr_inner_scheduler = TimeRecorder::new("SyMCTSScheduler::next::0-inner scheduler");
            let inner_next = self.inner_scheduler.next(state)?;
            let _ignored = inner_next; // we don't actually use the inner scheduler's choice, we use our own. We just let it track data.
        }

        assert!(!state.corpus().is_empty());
        let (_rand, corpus, state_metadata) = state.get_state_components_rand_corpus_metadata();
        let global_meta: &mut SyMCTSGlobalMetadata =
            state_metadata.get_mut::<SyMCTSGlobalMetadata>().unwrap();
        let global_time_spent_tracing = global_meta.total_time_spent_tracing_millis;

        let queue_len = global_meta.synced_inputs_queue.len();
        global_meta.synced_inputs_queue.rotate_right(min(10, queue_len));
        global_meta.synced_inputs_queue.truncate(10);

        let _current_tick = global_meta.total_num_times_sampled;

        let tr_get_covered_ids = TimeRecorder::new("SyMCTSScheduler::next::1-get_covered_ids");
        let covered_ids = global_meta
            .covered_branch_indices()
            .filter_map(|branch_idx| {
                let cov_info = &global_meta.coverage_point_info[branch_idx];
                if let Some(info) = cov_info {
                    if info.filtered_covering_corpus_ids(|&id| num_times_mutated(corpus, id) == 0).len() > 0 {
                        return Some(branch_idx);
                    }
                }
                None
            });
        drop(tr_get_covered_ids);

        #[cfg(feature="scheduling_weight_function_sampling_counts")]
        let weight_function = |branch_idx: &usize|  {
            let cov_info = global_meta.coverage_point_info[*branch_idx].as_ref().unwrap();
            let weight_sampled = (cov_info.num_times_symbolically_sampled * 100).pow(2);
            let weight_traced = cov_info.num_times_coverage_traced;
            // let ticks_since_last_seen = (current_tick - cov_info.tick_last_seen_mutated);
            weight_sampled + weight_traced
        };
        #[cfg(feature="scheduling_weight_function_time_spent")]
        let weight_function = |branch_idx: &usize|  {
            let cov_info = global_meta.coverage_point_info[*branch_idx].as_ref().unwrap();
            log::debug!("time_spent_tracing_millis: {}, total_time_spent_tracing: {}",
                cov_info.time_spent_tracing_millis,
                global_time_spent_tracing
            );
            let time_score = 1000usize.saturating_sub(((cov_info.time_spent_tracing_millis as f64 / global_time_spent_tracing.max(1) as f64) * 1000.) as usize);
            time_score // the less absolute time spent, the higher the score, the more likely to be picked
        };
        // #[cfg(feature="scheduling_weight_function_percent_unmutated")]
        // let weight_function = |branch_idx: &usize| {
        //     let cov_info = global_meta.coverage_point_info[*branch_idx].as_ref().unwrap();
        //     let numerator = cov_info.filtered_covering_corpus_ids(|&id| num_times_mutated(corpus, id) == 0).len();
        //     let denominator = cov_info.coverage_min_max_tracker.corpus().len();
        //     ((numerator as f64 / denominator as f64) * 10000.) as usize
        // };
        #[cfg(feature="scheduling_weight_function_least_unmutated")]
        let weight_function = |branch_idx: &usize| {
            let cov_info = global_meta.coverage_point_info[*branch_idx].as_ref().unwrap();
            let num_mutated = cov_info.filtered_covering_corpus_ids(|&id| num_times_mutated(corpus, id) > 0).len();
            let num_not_mutated = cov_info.coverage_min_max_tracker.corpus().len() - num_mutated;
            num_not_mutated
        };

        let tr_scheduler_select_coverage_point = TimeRecorder::new("SyMCTSScheduler::next::2-scheduler-select-coverage-point");
        let tr_scheduler_select_coverage_point_collect_covered_ids = TimeRecorder::new("SyMCTSScheduler::next::2-scheduler-select-coverage-point::collect_ids");
        let mut ids = covered_ids
            .collect::<Vec<_>>();
        drop(tr_scheduler_select_coverage_point_collect_covered_ids);

        let tr_scheduler_select_coverage_point_shuffle = TimeRecorder::new("SyMCTSScheduler::next::2-scheduler-select-coverage-point::shuffle");
        ids.shuffle(&mut rand::thread_rng());
        drop(tr_scheduler_select_coverage_point_shuffle);

        #[cfg(feature="scheduling_weighted_random")]
        let max_weight = ids.iter().map(weight_function).max().unwrap_or(0) + 1;

        let tr_scheduler_print = TimeRecorder::new("SyMCTSScheduler::next::2-scheduler-select-coverage-point::log");
        log::info!(target: "symcts_scheduler", "Scheduling among {} coverage points.", ids.len());
        log::info!(target: "symcts_scheduler", "Max weight among them: {}", max_weight);
        log::info!(target: "symcts_scheduler", "Weights: {:?}", ids.iter().map(weight_function).collect::<Vec<_>>());
        drop(tr_scheduler_print);
        // println!("ids={:?}", ids);

        let tr_scheduler_get_scheduled_branch_index = TimeRecorder::new("SyMCTSScheduler::next::2-scheduler-select-coverage-point::select_weighted");
        #[cfg(feature="scheduling_weighted_minimum")]
        let scheduled_branch_idx = ids
            .into_iter()
            .min_by_key(weight_function);

        #[cfg(feature="scheduling_weighted_random")]
        let scheduled_branch_idx = ids
            .choose_weighted_mut(&mut rand::thread_rng(), |x| max_weight - weight_function(x))
            .ok()
            .copied()
        ;

        #[cfg(feature="scheduling_uniform_random")]
        let scheduled_branch_idx = ids.into_iter().next();

        log::debug!(target: "symcts_scheduler", "scheduled: {:?}", scheduled_branch_idx);
        drop(tr_scheduler_get_scheduled_branch_index);
        drop(tr_scheduler_select_coverage_point);

        let sched_log_path = global_meta.sync_dir.join(".scheduler.log");
        let cur_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

        if let Some(scheduled_branch_idx) = scheduled_branch_idx {
            let scheduled_coverage_point = CoveragePoint { branch_index: scheduled_branch_idx, bucketed_count: 0 };

            // Extract the data we need from coverage_info before using global_meta elsewhere
            let (untraced_corpus, time_spent_tracing, num_times_traced, num_times_sampled, all_corpus_ids) = {
                let scheduled_coverage_info = global_meta.get_coverage_info(scheduled_branch_idx).unwrap();
                (
                    scheduled_coverage_info.filtered_covering_corpus_ids(|&id| num_times_mutated(corpus, id) == 0),
                    scheduled_coverage_info.time_spent_tracing_millis,
                    scheduled_coverage_info.num_times_coverage_traced,
                    scheduled_coverage_info.num_times_symbolically_sampled,
                    scheduled_coverage_info.coverage_min_max_tracker.as_ref().expect("should be set in on_add").corpus().clone(),
                )
            };

            let tr_scheduler_select_corpus_entry = TimeRecorder::new("SyMCTSScheduler::next::3-scheduler-select-corpus-entry");
            //////////////////////////////////////////////
            // println!("scheduled_coverage_info={:?}", scheduled_coverage_info);
            // randomly pick a corpusid from scheduled_coverage_info.coverage_min_max_tracker.corpus()

            log::debug!("avail_corpus={:?}", untraced_corpus
                .iter()
                .map(|&id| format!("{:?} => {:?}", id, corpus.get(id).unwrap().borrow().metadata::<MutationResultMetadata>()))
                .collect::<Vec<_>>()
            );

            let tr_scheduler_get_untraced_corpus = TimeRecorder::new("SyMCTSScheduler::next::4-get-untraced-corpus");
            let untraced_corpus = untraced_corpus
                .into_iter()
                .map(|id| {
                    let trace_len = trace_len(corpus, id);
                    let (input_length, exec_time) = input_length_and_exec_time(corpus, id).expect(format!("Failed to get input_length_and_exec_time for id {:?}", id).as_str());

                    (id, trace_len, input_length, exec_time)
                })
                .collect::<Vec<_>>();
            drop(tr_scheduler_get_untraced_corpus);
            let minmax_result = untraced_corpus.iter().map(|x| x.1).minmax();
            let least_covered_id = match minmax_result {
                MinMaxResult::NoElements => panic!("no elements in untraced corpus"),
                MinMaxResult::OneElement(_x) => {
                    let id = untraced_corpus.choose(&mut rand::thread_rng()).unwrap().0;
                    id
                },
                MinMaxResult::MinMax(min, max) => {
                    assert!(min <= max);
                    // let id = untraced_corpus.choose_weighted(&mut rand::thread_rng(), |x| {
                    //     1 + max - x.1
                    // }).unwrap().0;
                    let weights = untraced_corpus.iter().map(|x| {
                        len_time_mul_score(x.3, x.2)
                    }).collect::<Vec<_>>();
                    let max_weight = weights.iter().cloned().fold(f64::NAN, f64::max);

                    let id = untraced_corpus
                        .choose_weighted(&mut rand::thread_rng(), |x| {
                            let exec_time_millis = x.3;
                            let input_len = x.2;
                            let score = max_weight - len_time_mul_score(exec_time_millis, input_len) + 1.;
                            let time_score = 1000usize.saturating_sub(((exec_time_millis as f64 / time_spent_tracing.max(1) as f64) * 1000.) as usize).max(1);
                            score * (time_score as f64)
                        }).unwrap().0;
                    id
                }
            };


            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(sched_log_path)
                .unwrap();
            file.write_all(format!("{}\t{}\t{}\t{:?}\t{:?}\t{:?}\n",
                cur_time,
                num_times_sampled,
                num_times_traced,
                scheduled_coverage_point,
                least_covered_id,
                &all_corpus_ids).as_bytes()
            ).unwrap();
            log::info!(
                target: "symcts_scheduler",
                "Picked: {:#?}: sampled={}, traced={} => {:#?} [#corpus: {:?}, #untraced: {:?}]",
                &scheduled_coverage_point,
                num_times_sampled,
                num_times_traced,
                &least_covered_id,
                all_corpus_ids.len(),
                untraced_corpus.len(),
            );

            global_meta.last_scheduled = Some((scheduled_coverage_point.clone(), least_covered_id.clone()));

            let (input_len, execution_time_millis) = input_length_and_exec_time(corpus, least_covered_id)?;
            log::info!(target: "symcts_scheduler", "Scheduling input {:?} with length {} and exec_time_millis {} (avg={})",
                least_covered_id,
                input_len,
                execution_time_millis,
                (time_spent_tracing as f64) / num_times_traced.max(1) as f64
            );

            register_symbolic_sampling_of_testcase(state, least_covered_id, input_len, execution_time_millis);
            return Ok(least_covered_id.clone());
        }
        else {
            // okay, so we completely ran out of things to solve. Let's just randomly pick inputs to solve for, while
            // we wait for a miracle input to be found, either by us or an external fuzzer.
            // we weight towards longer inputs because there's likely more possible mutations there.
            log::warn!(
                target: "symcts_scheduler",
                "No coverage points left to solve for. Picking random input to continue."
            );
            let corpus = state.corpus();
            let corpus_ids = corpus.ids().map(|id| (id, input_len(corpus, id))).collect::<Vec<_>>();

            let (corpus_id, _length) = corpus_ids
                .choose_weighted(&mut rand::thread_rng(), |x| x.1)
                .expect("How can there be no inputs at all in the corpus??");

            let (input_len, execution_time_millis) = input_length_and_exec_time(corpus, *corpus_id)?;
            register_symbolic_sampling_of_testcase(state, *corpus_id, input_len, execution_time_millis);
            return Ok(*corpus_id)
        }
    }

    fn on_add(&mut self, state: &mut S, inserted_idx: CorpusId) -> Result<(), libafl::Error> {
        let tr_scheduler_on_add_full = TimeRecorder::new("SyMCTSScheduler::on_add");

        let tr_scheduler_on_add_inner = TimeRecorder::new("SyMCTSScheduler::on_add::0-inner-scheduler");
        self.inner_scheduler.on_add(state, inserted_idx)?;
        drop(tr_scheduler_on_add_inner);

        let tr_scheduler_get_testcase_info = TimeRecorder::new("SyMCTSScheduler::on_add::1-get-testcase-info");
        let (input_hash, input_len, execution_time_millis) = {
            let corpus = state.corpus();
            let mut testcase = corpus.get(inserted_idx).unwrap().borrow_mut();
            let exec_time = testcase.exec_time().map_or(1, |d| d.as_millis()) as u64;
            let input_len = testcase.load_len(corpus)? as u64;
            let input_hash = hash_target_bytes_input(testcase.load_input(corpus)?);
            (input_hash, input_len, exec_time)
        };
        drop(tr_scheduler_get_testcase_info);

        let global_meta = state
            .metadata_mut::<SyMCTSGlobalMetadata>()
            .expect("No global metadata set??");

        global_meta.hash_to_corpus_id.insert(input_hash, inserted_idx);

        let (cov_summary, single_cov, _branches_before) = global_meta
            .last_traced_cov
            .take()
            .expect("The scheduler on_add should only run after the feedback has populated its last_cov.");

        let tr_scheduler_register_new_inputs = TimeRecorder::new("SyMCTSScheduler::on_add::2-register-new-interesting-inputs");
        register_new_interesting_inputs(state, vec![(inserted_idx, cov_summary, single_cov, input_len as usize, execution_time_millis as usize)]);
        drop(tr_scheduler_register_new_inputs);
        state
            .corpus_mut()
            .get(inserted_idx)
            .unwrap()
            .borrow_mut()
            .add_metadata(MutationResultMetadata::default());
        Ok(())
    }

    /// Set current fuzzed corpus id and `scheduled_count`
    fn set_current_scheduled(
        &mut self,
        state: &mut S,
        next_idx: Option<CorpusId>,
    ) -> Result<(), Error> {
        self.inner_scheduler.set_current_scheduled(state, next_idx)?;

        let current_idx = *state.corpus().current();

        if let Some(idx) = current_idx {
            let mut testcase = state.testcase_mut(idx)?;
            let scheduled_count = testcase.scheduled_count();

            // increase scheduled count, this was fuzz_level in afl
            testcase.set_scheduled_count(scheduled_count + 1);
        }

        *state.corpus_mut().current_mut() = next_idx;
        Ok(())
    }
}
