//! The [`SyncFromAFLStage`] is a stage that imports inputs from disk for e.g. sync with AFL

use core::marker::PhantomData;
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use libafl::{inputs::HasTargetBytes};
use serde::{Deserialize, Serialize};

use libafl_bolts::{impl_serdeany};
use libafl::{
    fuzzer::{Evaluator},
    inputs::{Input},
    stages::Stage,
    common::HasMetadata,
    state::{HasClientPerfMonitor, HasCorpus, HasRand},
    Error,
};

use crate::{metadata::global::SyMCTSGlobalMetadata, util::hash_target_bytes_input};

/// Metadata used to store information about disk sync time
#[derive(Serialize, Deserialize, Debug)]
pub struct SyncFromDiskMetadata {
    /// The last time the sync was done
    pub last_time: SystemTime,
}

impl_serdeany!(SyncFromDiskMetadata);

impl SyncFromDiskMetadata {
    /// Create a new [`struct@SyncFromDiskMetadata`]
    #[must_use]
    pub fn new(last_time: SystemTime) -> Self {
        Self { last_time }
    }
}

/// A stage that loads testcases from disk to sync with other fuzzers such as AFL++
#[derive(Debug)]
pub struct SyncFromAFLStage<I, S, CB, E, EM, Z> {
    sync_dir: PathBuf,
    load_callback: CB,
    phantom: PhantomData<(I, S, E, EM, Z)>,
}

impl<CB, E, EM, I, S, Z> Stage<E, EM, S, Z> for SyncFromAFLStage<I, S, CB, E, EM, Z>
where
    CB: FnMut(&mut Z, &mut S, &Path) -> Result<I, Error>,
    Z: Evaluator<E, EM, I, S>,
    S: HasClientPerfMonitor + HasCorpus<I> + HasRand + HasMetadata,
    I: HasTargetBytes,
{
    #[inline]
    fn perform(
        &mut self,
        fuzzer: &mut Z,
        executor: &mut E,
        state: &mut S,
        manager: &mut EM,
    ) -> Result<(), Error> {
        let global_meta = state.metadata_mut::<SyMCTSGlobalMetadata>().unwrap();

        let current_tick = global_meta.current_tick();
        let last_tick_seen_new_branch = global_meta.last_tick_seen_new_branch;
        eprintln!("SYNC: current_tick: {}, last_tick_seen_new_branch: {}", current_tick, last_tick_seen_new_branch);
        #[cfg(feature = "sync_only_when_stuck")]
        if !global_meta.seems_stuck() {
            return Ok(());
        }
        global_meta.reset_stuck_counter(); // reset the stuck counter to not immediately sync again

        let last_synced_time = state
            .metadata::<SyncFromDiskMetadata>()
            .ok()
            .map(|m| m.last_time);

        let now = SystemTime::now();
        if let Some(l) = last_synced_time {
            if now.duration_since(l).unwrap().as_secs() < 120 {
                return Ok(());
            }
        }
        let path = self.sync_dir.clone();
        if let Some(max_time) =
            self.load_from_directory(&path, &last_synced_time, fuzzer, executor, state, manager)?
        {
            if last_synced_time.is_none() {
                state
                    .add_metadata(SyncFromDiskMetadata::new(max_time));
            } else {
                state
                    .metadata_mut::<SyncFromDiskMetadata>()
                    .unwrap()
                    .last_time = max_time;
            }
        }

        #[cfg(feature = "introspection")]
        state.introspection_stats_mut().finish_stage();

        Ok(())
    }
}

impl<CB, E, EM, I, S, Z> SyncFromAFLStage<I, S, CB, E, EM, Z>
where
    CB: FnMut(&mut Z, &mut S, &Path) -> Result<I, Error>,
    Z: Evaluator<E, EM, I, S>,
    S: HasClientPerfMonitor + HasCorpus<I> + HasRand + HasMetadata,
    I: HasTargetBytes,
{
    /// Creates a new [`SyncFromAFLStage`]
    #[must_use]
    pub fn new(sync_dir: PathBuf, load_callback: CB) -> Self {
        Self {
            sync_dir,
            load_callback,
            phantom: PhantomData,
        }
    }

    fn load_from_directory(
        &mut self,
        in_dir: &Path,
        last: &Option<SystemTime>,
        fuzzer: &mut Z,
        executor: &mut E,
        state: &mut S,
        manager: &mut EM,
    ) -> Result<Option<SystemTime>, Error> {
        let mut max_time = None;
        log::info!(target: "sync_from_afl_stage", "Loading from directory: {:?}", in_dir);
        let my_sync_dir = state.metadata::<SyMCTSGlobalMetadata>().unwrap().sync_dir.canonicalize()?;
        for entry in fs::read_dir(in_dir)?.collect::<Vec<_>>().into_iter() {
            let entry = entry?;
            let path = match entry.path().canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    log::warn!(target: "sync_from_afl_stage", "Failed to canonicalize path: {:?}: {:?}", entry.path(), e);
                    continue;
                }
            };
            log::debug!(target: "sync_from_afl_stage", "Found path: {:?} => {:?}", path, entry);

            if path.file_name().unwrap().to_str().unwrap().starts_with(".") {
                continue;
            }

            let attributes = fs::metadata(&path);

            if attributes.is_err() {
                continue;
            }

            let attr = attributes?;

            if attr.is_file() && attr.len() > 0 {
                if let Ok(time) = attr.modified() {
                    log::debug!(target: "sync_from_afl_stage", "Checking time: {:?} > {:?}", time, last);
                    if let Some(l) = last {
                        if time.duration_since(*l).is_err() {
                            continue;
                        }
                    }

                    // if it doesn't start with 'id:', it's not a testcase, so skip it
                    if !path.file_name().unwrap().to_str().unwrap().starts_with("id:") {
                        continue;
                    }

                    log::info!(target: "sync_from_afl_stage", "Loading: {:?}", path);
                    max_time = Some(max_time.map_or(time, |t: SystemTime| t.max(time)));
                    let input = (self.load_callback)(fuzzer, state, &path)?;
                    let hash = hash_target_bytes_input(&input);
                    if state.metadata::<SyMCTSGlobalMetadata>().unwrap().hash_to_corpus_id.contains_key(&hash) {
                        log::warn!(target: "sync_from_afl_stage", "Skipping input with duplicate hash: {:?}", path);
                        continue;
                    }
                    else {
                        if let (res, Some(corpus_id)) = fuzzer.evaluate_input(state, executor, manager, &input)? {
                            if res.is_corpus() {
                                let global_meta = state.metadata_mut::<SyMCTSGlobalMetadata>().unwrap();
                                global_meta.synced_inputs_queue.push(corpus_id);
                                global_meta.hash_to_corpus_id.insert(hash, corpus_id);
                            }
                        }
                    }
                }
            } else if attr.is_dir() {
                // if the name is symcts_latest, skip it
                if path.file_name().unwrap().to_str().unwrap() == "symcts_latest" {
                    continue;
                }
                // if it's my_sync_dir, skip it
                if path.starts_with(&my_sync_dir) {
                    continue;
                }
                let dir_max_time =
                    self.load_from_directory(&path, last, fuzzer, executor, state, manager)?;
                if let Some(time) = dir_max_time {
                    max_time = Some(max_time.map_or(time, |t: SystemTime| t.max(time)));
                }
            }
        }

        Ok(max_time)
    }
}

/// Function type when the callback in `SyncFromAFLStage` is not a lambda
pub type SyncFromDiskFunction<I: Input, S, Z> =
    fn(&mut Z, &mut S, &Path) -> Result<I, Error>;

impl<E, EM, I, S, Z> SyncFromAFLStage<I, S, SyncFromDiskFunction<I, S, Z>, E, EM, Z>
where
    I: Input,
    S: HasCorpus<I>,
    Z: Evaluator<E, EM, I, S>,
    S: HasClientPerfMonitor + HasCorpus<I> + HasRand + HasMetadata,
{
    /// Creates a new [`SyncFromAFLStage`] invoking `Input::from_file` to load inputs
    #[must_use]
    pub fn with_from_file(sync_dir: PathBuf) -> Self {
        fn load_callback<Z, S, I: Input>(
            _: &mut Z,
            _: &mut S,
            p: &Path,
        ) -> Result<I, Error> {
            Input::from_file(p)
        }
        Self {
            sync_dir,
            load_callback: load_callback::<_, _, _>,
            phantom: PhantomData,
        }
    }
}
