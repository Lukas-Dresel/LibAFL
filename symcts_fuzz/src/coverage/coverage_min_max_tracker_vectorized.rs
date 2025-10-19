use bitvec::vec::BitVec;

use itertools::Itertools;
use libafl::prelude::CorpusId;
use serde::{Deserialize, Serialize, ser::SerializeStruct};
use core::panic;
use std::{fmt::Debug, ops::{BitOrAssign, BitOr}};

use crate::coverage::vectorized_coverage_map::CounterCondMask;
use crate::util::TimeRecorder;

use super::vectorized_coverage_map::{LANES, MinimizingVectorizedCounter, MaximizingVectorizedCounter, VectorizedCoverage, MIN_COUNT};
use super::InterestReason;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CoverageMinMaxTracker {
    pub present_bitmap: BitVec,
    pub map: Vec<(MinimizingVectorizedCounter, MaximizingVectorizedCounter)>,
    pub longest_input_length_exponent_seen: usize,
    pub corpus: Vec<CorpusId>,
}

// impl Serialize for CoverageMinMaxTracker {
//     fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: serde::Serializer,
//     {
//         let interesting_entries = self.interesting_entries().into_iter().enumerate().collect::<Vec<_>>();

//         let mut object = serializer.serialize_struct("CoverageMinMaxTracker", 3)?;
//         object.serialize_field("map_entries_len", &self.map.len())?;
//         object.serialize_field("interesting_entries", &interesting_entries)?;
//         object.serialize_field("corpus", &self.corpus)?;
//         object.serialize_field("longest_input_length_seen", &self.longest_input_length_exponent_seen)?;

//         object.end()
//     }
// }
// impl<'de> Deserialize<'de> for CoverageMinMaxTracker {
//     fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
//     where
//         D: serde::Deserializer<'de>,
//     {
        
//     }
// }

impl Debug for CoverageMinMaxTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoverageMinMaxTracker")
            .field("map_entries", &self.map.len())
            .field("map_entries_interesting", &self.map.iter().filter(|(_min, _max)| {
                !_min.is_uninteresting() && !_max.is_uninteresting()
            }).count())
            .field("corpus", &self.corpus)
            .finish()
    }
}

impl CoverageMinMaxTracker {
    pub fn create(initial_counts: &VectorizedCoverage, initial_corpus_id: CorpusId) -> Self {
        Self {
            longest_input_length_exponent_seen: initial_counts.input_length_exponent,
            present_bitmap: BitVec::from_iter(initial_counts.map.iter().map(|&x| x != MIN_COUNT)),
            map: initial_counts
                 .map
                 .iter()
                 .map(|&x| (x.into(), x.into()))
                 .collect_vec(),
            corpus: vec![initial_corpus_id],
        }
    }
    fn interesting_entries(&self) -> Vec<(usize, &(MinimizingVectorizedCounter, MaximizingVectorizedCounter))> {
        assert!(self.present_bitmap.len() == self.map.len());
        self.present_bitmap.iter_ones().map(|i| (i, &self.map[i])).collect_vec()
    }

    pub fn is_interesting_for(&self, coverage: &VectorizedCoverage) -> Option<InterestReason> {
        // if coverage.input_length_exponent > self.longest_input_length_exponent_seen {
        //     return Some(InterestReason::Longest { old: self.longest_input_length_exponent_seen, new: coverage.input_length_exponent });
        // }
        let tr_full = TimeRecorder::new("CoverageMinMaxTracker::is_interesting_for");
        assert!(coverage.num_vectored_entries() == self.map.len() || self.map.len() == 0);
        if self.map.len() == 0 {
            // here we only have to iterate over the non-zeros in the input, we know for sure they are interesting
            let tr_fastpath_empty_map = TimeRecorder::new("CoverageMinMaxTracker::is_interesting_for::fastpath_empty_map");
            return coverage
                .non_zero_bitmap
                .iter_ones()
                .next()
                .map(|i| {
                    let (non_zero_index, non_zero_val) = coverage.map[i].to_array().into_iter().enumerate().find(|(_, x)| *x != 0).unwrap();
                    InterestReason::Maximizes { index: i * LANES + non_zero_index, old: 0, new: non_zero_val.into() }
                });
        }
        // iterator over the zip of self.map and coverage_simd, however, extended to the max length of either
        // this is because we want to iterate over the longest one

        // okay, so this is going to be counter intuitive, but we try to optimize for the general case (no improvement)
        // in that case, we *have* to iterate over the entire map anyways.
        // so let's do that now, doing a quick bitwise or on the map only to detect if any improvements were made
        // if not, this is the fastest possible way out

        #[cfg(feature="coverage_fastpath_no_change_case")]
        {
            let tr_fastpath_no_change_case = TimeRecorder::new("CoverageMinMaxTracker::is_interesting_for::fastpath_no_change_case");
            let mut is_interesting = CounterCondMask::splat(false);

            // Iterate through bitmap chunks and OR them on-the-fly to avoid allocation
            let self_storage = self.present_bitmap.as_raw_slice();
            let coverage_storage = coverage.non_zero_bitmap.as_raw_slice();
            let max_chunks = self_storage.len().max(coverage_storage.len());

            for chunk_idx in 0..max_chunks {
                let self_chunk = self_storage.get(chunk_idx).copied().unwrap_or(0);
                let cov_chunk = coverage_storage.get(chunk_idx).copied().unwrap_or(0);
                let union_chunk = self_chunk | cov_chunk;

                if union_chunk == 0 {
                    continue; // Skip empty chunks
                }

                // Iterate over set bits in this chunk
                let mut remaining = union_chunk;
                while remaining != 0 {
                    let bit_offset = remaining.trailing_zeros() as usize;
                    let pos = chunk_idx * (usize::BITS as usize) + bit_offset;

                    if pos < self.map.len() {
                        let (min_ent, max_ent) = &self.map[pos];
                        let cur_ent = coverage.map[pos];
                        is_interesting |= MinimizingVectorizedCounter::is_better_combined(min_ent, max_ent, cur_ent);
                    }

                    remaining &= remaining - 1; // Clear lowest set bit
                }
            }

            if !is_interesting.any() {
                return None; // the most common case, no improvements anywhere, just exit out
            }
        }

        // then, in the rare case that we do see an improvement, we have to do it again, to find where the improvement
        // happened
        let tr_detailed_check = TimeRecorder::new("CoverageMinMaxTracker::is_interesting_for::detailed_check");

        // Use the same chunked iteration approach
        let self_storage = self.present_bitmap.as_raw_slice();
        let coverage_storage = coverage.non_zero_bitmap.as_raw_slice();
        let max_chunks = self_storage.len().max(coverage_storage.len());

        for chunk_idx in 0..max_chunks {
            let self_chunk = self_storage.get(chunk_idx).copied().unwrap_or(0);
            let cov_chunk = coverage_storage.get(chunk_idx).copied().unwrap_or(0);
            let union_chunk = self_chunk | cov_chunk;

            if union_chunk == 0 {
                continue;
            }

            let mut remaining = union_chunk;
            while remaining != 0 {
                let bit_offset = remaining.trailing_zeros() as usize;
                let pos = chunk_idx * (usize::BITS as usize) + bit_offset;

                if pos < self.map.len() {
                    let (min_ent, max_ent) = &self.map[pos];
                    let cur_ent = coverage.map[pos];

                    let tr_detailed_check_minimizes = TimeRecorder::new("CoverageMinMaxTracker::is_interesting_for::detailed_check::minimizes");
                    let minimizes = min_ent.is_better(cur_ent);

                    // fast path out ASAP if at all possible
                    if minimizes.any() {
                        let index_min = minimizes.to_array().iter().position(|&x| x).unwrap();
                        return Some(InterestReason::Minimizes {
                            index: LANES * pos + index_min,
                            old: min_ent.get().to_array()[index_min].into(),
                            new: cur_ent.to_array()[index_min].into(),
                        });
                    }
                    drop(tr_detailed_check_minimizes); // log time

                    let tr_detailed_check_maximizes = TimeRecorder::new("CoverageMinMaxTracker::is_interesting_for::detailed_check::maximizes");
                    let maximizes = max_ent.is_better(cur_ent);
                    if maximizes.any() {
                        let index_max = maximizes.to_array().iter().position(|&x| x).unwrap();
                        return Some(InterestReason::Maximizes {
                            index: LANES * pos + index_max,
                            old: max_ent.get().to_array()[index_max].into(),
                            new: cur_ent.to_array()[index_max].into(),
                        });
                    }
                    drop(tr_detailed_check_maximizes); // log time
                }

                remaining &= remaining - 1;
            }
        }
        return None;
    }

    pub fn add(&mut self, coverage: &VectorizedCoverage, corpus_id: CorpusId) {
        assert!(self.map.len() == coverage.num_vectored_entries() || self.map.len() == 0);

        if self.map.len() == 0 {
            self.present_bitmap = coverage.non_zero_bitmap.clone();
            self.map = coverage
                .coverage_map()
                .iter()
                .map(|&x| {
                    (
                        x.into(),
                        x.into(),
                    )
                })
                .collect();
            self.longest_input_length_exponent_seen = coverage.input_length_exponent;
            self.corpus.push(corpus_id);
            return;
        }

        let mut novel = false;
        if coverage.input_length_exponent > self.longest_input_length_exponent_seen {
            self.longest_input_length_exponent_seen = coverage.input_length_exponent;
            novel = true;
        }

        self.present_bitmap.bitor_assign(&coverage.non_zero_bitmap);
        for pos in self.present_bitmap.iter_ones() {
            let (min_ent, max_ent) = &mut self.map[pos];
            let cur_ent = coverage.map[pos];
            let minimizes = min_ent.is_better(cur_ent);
            let maximizes = max_ent.is_better(cur_ent);
            if minimizes.any() {
                min_ent.update_min(cur_ent);
                novel = true;
            }
            if maximizes.any() {
                max_ent.update_max(cur_ent);
                novel = true;
            }
        }
        if novel {
            self.corpus.push(corpus_id);
        }
    }

    pub fn corpus(&self) -> &Vec<CorpusId> {
        &self.corpus
    }

    pub fn filtered_covering_corpus_ids(&self, predicate: impl Fn(&CorpusId) -> bool) -> Vec<CorpusId> {
        self.corpus
            .iter()
            .filter(|&&corpus_id| predicate(&corpus_id))
            .copied()
            .collect_vec()
    }

}
