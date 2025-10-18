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

use libafl::inputs::HasTargetBytes;
use libafl_bolts::prelude::AsSlice;

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

// Re-export time recording utilities from libafl_bolts
pub use libafl_bolts::timerecorder::{
    TimeRecorder,
    add_time_for_slot,
    dump_total_time_snapshot,
    dump_percentage_time_snapshot,
};