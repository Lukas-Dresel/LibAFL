//! This is a basic SymCC runtime.
//! It traces the execution to the shared memory region that should be passed through the environment by the fuzzer process.
//! Additionally, it concretizes all floating point operations for simplicity.
//! Refer to the `symcc_runtime` crate documentation for building your own runtime.

use symcc_runtime::{
    export_runtime,
    filter::{CallStackCoverage, NoFloat, NoMem},
    tracing::{self, StdShMemMessageFileWriter},
    Runtime,
};

export_runtime!(
    NoFloat => NoFloat;
    NoMem => NoMem;
    CallStackCoverage::default() => CallStackCoverage; // QSym-style expression pruning
    tracing::TracingRuntime::new(
        if std::env::var("SYMCC_DISABLE_WRITING").is_ok() {
            // eprintln!("SYMCC_DISABLE_WRITING is set, not writing to shared memory");
            None
        } else {
            Some(StdShMemMessageFileWriter::from_stdshmem_default_env().unwrap_or_else(|err| {
                        eprintln!("unable to construct tracing runtime writer. (missing env?) reason={}", err);
                        eprintln!("If you want to disable writing to shared memory (e.g. when running testcases during compilation where shared memory is unavailable), you can set the environment variable SYMCC_DISABLE_WRITING to allow this.");
                        panic!("unable to construct tracing runtime writer");
                    }))
        },
        std::env::var("SYMCC_TRACE_LOCATIONS").is_ok(),
        std::env::var("SYMCC_PRINT_MESSAGES").is_ok(),
        std::env::var("SYMCC_TRACE_BEFORE_SYMBOLIC").is_ok(),
    ) => tracing::TracingRuntime
);
