//! A libfuzzer-like fuzzer with llmp-multithreading support and restarts
//! The example harness is built for `stb_image`.

#![feature(portable_simd)]
#![feature(iter_array_chunks)]

pub mod symcts_mutational_stage;
pub mod symcts_scheduler;

#[cfg(feature = "scheduling_symcc")]
pub mod symcc_scheduler;
// pub mod symcts_corpus;
pub mod metadata;
pub mod coverage;
pub mod util;
pub mod disk_backed_concolic_metadata;
pub mod sync_from_afl_stage;
pub mod reproducibility_details;
pub mod standalone_cov_tracer;
pub mod interpreted_trace;
pub mod symcts_mutations;

#[cfg(feature = "resource_tracking")]
pub mod resource_monitoring;