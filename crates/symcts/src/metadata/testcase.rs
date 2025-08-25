//! Map feedback, maximizing or minimizing maps, for example the afl-style map observer.
use crate::coverage::CoverageSummary;
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

// TODO maybe use bignums instead of just usize, for now use `checked_add` to see if necessary
// #[derive(Default, Debug, Serialize, Deserialize, Clone)]
// pub struct SyMCTSTestcaseMetadata {
//     // pub coverage_summary: CoverageSummary,
//     // pub location_vec: Vec<Location>,
//     pub input_size: usize,
// }

// // TODO come up with a way to have fast performant checking of novel inputs and branch iteration.
// // E.g. I was thinking about having GlobalMetadata have an order assignment of branches to indices and then
// // computing a bitmap representation of the coverage points. This way a check for novelty would be a simple & operation
// libafl_bolts::impl_serdeany!(SyMCTSTestcaseMetadata);
