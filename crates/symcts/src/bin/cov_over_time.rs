//! A libfuzzer-like fuzzer with llmp-multithreading support and restarts
//! The example harness is built for `stb_image`.

#![feature(option_get_or_insert_default)]

use bitvec::vec::BitVec;
use plotlib::{repr::Plot, style::{LineStyle, LineJoin}, view::ContinuousView, page::Page};
use plotters::prelude::*;

use libafl_targets::coverage;
use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use itertools::Itertools;
use symcts::coverage::SyMCTSCoverageFeedback;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio}, marker::PhantomData, cmp::max, time::SystemTime,
};
use symcts::symcts_scheduler::SyMCTSScheduler;

use libafl::{
    bolts::{
        current_nanos,
        rands::StdRand,
        shmem::{ShMem, ShMemProvider, StdShMemProvider},
        tuples::{tuple_list},
        AsSlice, AsMutSlice,
    },
    corpus::{Corpus, InMemoryCorpus},
    events::{SimpleEventManager, Event, EventFirer},
    executors::{command::CommandConfigurator, ForkserverExecutor},
    feedbacks::CrashFeedback,
    fuzzer::StdFuzzer,
    inputs::{BytesInput, HasTargetBytes, Input},
    monitors::{MultiMonitor, UserStats},
    observers::{
        concolic::{
            serialization_format::{DEFAULT_ENV_NAME, DEFAULT_SIZE},
            ConcolicObserver,
        }, HitcountsMapObserver, StdMapObserver, TimeObserver,
    },
    state::{HasCorpus, HasMetadata, StdState},
    Error, prelude::{Executor, Rand, HasObservers, ExitKind, Observer, MapObserver},
};
use symcts::metadata::global::{register_new_interesting_inputs, SyMCTSGlobalMetadata};
use clap::{self, Parser};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// directory containing testcase
    #[clap()]
    directory: String,
    #[clap(multiple = true)]
    pub target_commandline: Vec<String>,
}

pub fn main() {
    let args = Args::parse();
    fuzz(args)
    .expect("An error occurred while fuzzing");
}

/// The actual fuzzer
fn fuzz(args: Args) -> Result<(), Error> {
    // 'While the stats are state, they are usually used in the broker - which is likely never restarted
    let monitor = MultiMonitor::new(|s| println!("{}", s));

    let mut rng = StdRand::with_seed(0);

    let mut shmem_provider = StdShMemProvider::new().unwrap();

    let time_observer = TimeObserver::new("time_coverage");

    const MAP_SIZE: usize = 65536;
    let mut afl_shm = shmem_provider
        .new_shmem(MAP_SIZE)
        .unwrap();
    afl_shm.write_to_env("__AFL_SHM_ID").unwrap();
    let afl_shm_id = afl_shm.id().to_string();

    let slice_of_u8s = afl_shm.as_mut_slice();
    // convert slice to slice of u32s
    let slice_of_u32s = unsafe {
        std::slice::from_raw_parts_mut(
            slice_of_u8s.as_mut_ptr() as *mut u32,
            slice_of_u8s.len() / std::mem::size_of::<u32>(),
        )
    };

    let afl_map_observer = unsafe {
        StdMapObserver::<u32, false>::new("afl_map", slice_of_u32s)
    };
    let mut symcts_feedback = SyMCTSCoverageFeedback::for_afl_bitmap_observer(&afl_map_observer);
    let mut crash_feedback = CrashFeedback::new();

    type StateType = StdState<BytesInput, InMemoryCorpus<BytesInput>, StdRand, InMemoryCorpus<BytesInput>>;

    let mut coverage_executor = ForkserverExecutor::builder()
        .program(&args.target_commandline[0])
        .debug_child(false)
        .shmem_provider(&mut shmem_provider)
        .is_persistent(true)
        .env("__AFL_SHM_ID", afl_shm_id)
        .env("AFL_MAP_size", DEFAULT_SIZE.to_string())
        .env("__AFL_OUT_DIR", "/tmp/afl_out")
        .parse_afl_cmdline(&args.target_commandline[1..])
        .build_dynamic_map::<StdMapObserver<'_, u32, false>, (), StateType>(afl_map_observer, ())?;

    // get all inputs in args.directory
    let inputs = std::fs::read_dir(&args.directory)?
        .into_iter()
        .filter_map(|entry| {
            let path = entry.expect("Could not read entry").path();

            // ignore hidden files
            if path.file_name().unwrap().to_str().unwrap().starts_with(".") {
                None
            }
            else if path.file_name().unwrap().to_str().unwrap().contains(",orig:") {
                // the original corpus files do not have their timestamps updated, so we ignore them
                None
            }
            else if path.is_file() {
                let created = path.metadata().expect("Could not get metadata?").created().expect("Could not get creation time?");
                let input = BytesInput::from_file(&path).expect("Could not load input");
                Some((path, created, input))
            } else {
                None
            }
        })
        .sorted_by_key(|x| x.1)
        .collect::<Vec<_>>();

    let mut triggered: BitVec = Default::default();
    let mut seen_adjacent: BitVec = Default::default();
    let mut seen_in_function: BitVec = Default::default();

    println!("timestamp, cov_hit, cov_adjacent, cov_function");
    let mut result = vec![];
    let (min_timestamp, max_timestamp) = (inputs.first().unwrap().1, inputs.last().unwrap().1);
    for (input_path, creation_time, input) in inputs.into_iter() {
        coverage_executor.observers_mut().0.reset_map().expect("Could not reset map");
        // println!("Running input: before: {:?}", map_repr(&coverage_executor.observers().0.as_slice()[1..]));
        let time_pre = SystemTime::now();
        let exit_kind = coverage_executor.execute_input(&input).expect("Could not run coverage??");
        let time_taken_tracing = SystemTime::now().duration_since(time_pre).unwrap();
        let observer_map = coverage_executor.observers().0.as_slice();
        eprintln!("Time taken tracing: {:?}", time_taken_tracing.as_millis());

        let triggered_bitvec = BitVec::from_iter(
                observer_map.iter().map(
                |x| (x & !(3u32 << 30)) != 0
            ));
        let adjacent_bitvec = BitVec::from_iter(
            observer_map.iter().map(
                |x| (x & (2u32 << 30)) != 0
            ));
        let function_adjacent_bitvec = BitVec::from_iter(
            observer_map.iter().map(
                |x| (x & (1u32 << 30)) != 0
            ));

        triggered.resize(max(triggered.len(), triggered_bitvec.len()), false);
        seen_adjacent.resize(max(seen_adjacent.len(), adjacent_bitvec.len()), false);
        seen_in_function.resize(max(seen_in_function.len(), function_adjacent_bitvec.len()), false);

        triggered |= triggered_bitvec;
        seen_adjacent |= adjacent_bitvec;
        seen_in_function |= function_adjacent_bitvec;

        let creation_time = creation_time.duration_since(min_timestamp).expect("Could not get duration since epoch");

        println!("{:#?}, {}, {}, {}", creation_time, triggered.count_ones(), seen_adjacent.count_ones(), seen_in_function.count_ones());
        result.push((input_path, creation_time, triggered.count_ones(), seen_adjacent.count_ones(), seen_in_function.count_ones()));
    }

    Ok(())
    // plot the results
}

#[derive(Default, Debug)]
pub struct SymCCTracerCommandConfigurator {
    with_symbolic_input: bool,
}

impl SymCCTracerCommandConfigurator {
    pub fn new(with_symbolic_input: bool) -> Self {
        SymCCTracerCommandConfigurator { with_symbolic_input }
    }
}

impl CommandConfigurator for SymCCTracerCommandConfigurator {
    fn spawn_child<I: Input + HasTargetBytes>(&mut self, input: &I) -> Result<Child, Error> {
        input.to_file("cur_input_print_cov")?;

        let mut cmd = Command::new("./target_symcts");
        cmd
            .arg("cur_input_print_cov")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if self.with_symbolic_input {
            cmd.env("SYMCC_INPUT_FILE", "cur_input_print_cov");
        }
        else {
            cmd.env("SYMCC_NO_SYMBOLIC_INPUT", "1");
        }

        Ok(cmd.spawn().expect("failed to start process"))
    }
}
