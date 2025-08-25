use std::cmp::min;
use std::io::Write;
use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use itertools::{Itertools, MinMaxResult};
use libafl::corpus::{Corpus, CorpusId};
use libafl::prelude::{UsesInput, HasTestcase, HasTargetBytes, HasBytesVec};
use libafl::schedulers::Scheduler;
use libafl::state::{HasCorpus, HasMetadata, HasRand, UsesState, BetterStateTrait};
use libafl::{Error};
use libafl::bolts::{HasLen, rands::Rand};
use rand::prelude::SliceRandom;

use crate::coverage::{CoveragePoint, CoverageSummary};
use crate::metadata::global::{SyMCTSGlobalMetadata, CoverageLocationInfo, register_new_interesting_inputs, register_symbolic_sampling_of_testcase};
use crate::symcts_mutations::MutationResultMetadata;
use crate::util::hash_target_bytes_input;
use crate::symcts_mutations::MutationSource;

/// Score of a test case.
///
/// We use the lexical comparison implemented by the derived implementation of
/// Ord in order to compare according to various criteria.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
pub struct TestcaseScore {
    /// First criterion: new coverage
    new_coverage: bool,

    /// Second criterion: being derived from seed inputs
    derived_from_seed: bool,

    /// Third criterion: size (smaller is better)
    file_size: i128,

    /// Fourth criterion: the ID
    corpus_id: CorpusId,
}

impl TestcaseScore {
    /// Score a test case.
    ///
    /// If anything goes wrong, return the minimum score.
    fn new(corpus_id: CorpusId, input_len: usize, hit_new_cov: bool, seed_input: bool) -> Self {
        let size: i128 = input_len.try_into().unwrap();

        TestcaseScore {
            new_coverage: hit_new_cov,
            derived_from_seed: seed_input,
            file_size: -i128::from(size),
            corpus_id,
        }
    }

    // /// Return the smallest possible score.
    // fn minimum() -> TestcaseScore {
    //     TestcaseScore {
    //         new_coverage: false,
    //         derived_from_seed: false,
    //         file_size: std::i128::MIN,
    //         base_name: OsString::from(""),
    //     }
    // }
}


#[derive(Debug, Default, Clone)]
pub struct SymCCScheduler<S> {
    seen: HashSet<CorpusId>,
    // ordered list of testcases to schedule
    scores_to_testcases: Vec<(TestcaseScore, CorpusId)>,
    phantom: PhantomData<S>,
}

impl<S> SymCCScheduler<S> {
    pub fn new() -> Self {
        Self {
            seen: HashSet::new(),
            scores_to_testcases: Vec::new(),
            phantom: PhantomData,
        }
    }
    pub fn best_new_testcase(&self) -> Option<(TestcaseScore, CorpusId)> {
        let best = self.scores_to_testcases
            .iter()
            .filter(|(score, corpus_id)| !self.seen.contains(corpus_id))
            .max_by_key(|(score, corpus_id)| score);

        best.cloned()
    }
}

fn input_len<C: Corpus<I>>(corpus: &C, id: CorpusId) -> usize 
where
    I: HasLen,
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

impl<S> Scheduler for SymCCScheduler<S>
where
    S: HasCorpus + HasRand + HasMetadata + HasTestcase + BetterStateTrait,
    S::Input: HasLen + HasBytesVec + HasTargetBytes,
{
    fn on_add(&mut self, state: &mut Self::State, inserted_idx: CorpusId) -> Result<(), libafl::Error> {
        let input_hash = {
            let mut testcase = state.corpus().get(inserted_idx).unwrap().borrow_mut();
            let hash = hash_target_bytes_input(testcase.load_input()?);
            hash
        };
        let global_meta = state
            .metadata_mut::<SyMCTSGlobalMetadata>()
            .expect("No global metadata set??");

        global_meta.hash_to_corpus_id.insert(input_hash, inserted_idx);
        let (cov_summary, single_cov, branches_before) = global_meta
            .last_traced_cov
            .take()
            .expect("The scheduler on_add should only run after the feedback has populated its last_cov.");

        let mutation_source = global_meta.current_mutation_source.clone();
        let is_seed_input = {
            global_meta.current_mutation_source.is_none()
        };

        register_new_interesting_inputs(state, vec![(inserted_idx, cov_summary, single_cov)]);

        let branches_after = {
            state.metadata::<SyMCTSGlobalMetadata>().unwrap().coverage_point_info.len()
        };
        // eprintln!("on_add: New testcase: {:?}, mutation_source: {:?}, branches_before: {}, branches_after: {}", inserted_idx, mutation_source, branches_before, branches_after);
        self.scores_to_testcases.push((
            TestcaseScore::new(
                inserted_idx,
                input_len(state.corpus(), inserted_idx),
                branches_after > branches_before,
                is_seed_input, // no mutation source means seed input
            ),
            inserted_idx,
        ));

        state
            .corpus_mut()
            .get(inserted_idx)
            .unwrap()
            .borrow_mut()
            .add_metadata(MutationResultMetadata::default());
        Ok(())
    }
    fn next(&mut self, state: &mut Self::State) -> Result<CorpusId, Error> {
        assert!(!state.corpus().is_empty());
        let (_rand, corpus, state_metadata) = state.get_state_components_rand_corpus_metadata();
        let global_meta: &mut SyMCTSGlobalMetadata =
            state_metadata.get_mut::<SyMCTSGlobalMetadata>().unwrap();

        // selected corpus id = best_new_testcase or random if none
        let (score, corpus_id) = self.best_new_testcase().unwrap_or_else(|| {
            let mut rng = state.rand_mut();
            let idx = rng.between(0, self.scores_to_testcases.len() as u64 - 1);
            self.scores_to_testcases[idx as usize].clone()
        });
        // eprintln!("Selected testcase: {:?}: {:?}", corpus_id, score);

        register_symbolic_sampling_of_testcase(
            state,
            corpus_id,
        );
        self.seen.insert(corpus_id);

        return Ok(corpus_id.clone());
    }


    /// Set current fuzzed corpus id and `scheduled_count`
    fn set_current_scheduled(
        &mut self,
        state: &mut Self::State,
        next_idx: Option<CorpusId>,
    ) -> Result<(), Error> {
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
