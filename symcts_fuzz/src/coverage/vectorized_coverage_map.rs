use std::simd::{prelude::{SimdPartialEq, SimdOrd, SimdPartialOrd}, Simd};
use bitvec::vec::BitVec;

use serde::{Deserialize, Serialize, ser::SerializeStruct};
use super::loop_bucketing::get_bucketed_hitcount_inner;

pub type InstrumentationCounterType = u8;
pub const INSTRUMENTATION_COUNTER_ZERO: InstrumentationCounterType = 0;
pub type CounterType = u8;
pub const COUNTER_TYPE_ZERO: CounterType = 0u8;
pub const LANES: usize = 32;
pub const MIN_SINGLE_COUNT: CounterType = 0;
#[cfg(feature = "symcts_32bit_counters")]
pub const MAX_SINGLE_COUNT: CounterType = 0xffff;
#[cfg(not(feature = "symcts_32bit_counters"))]
pub const MAX_SINGLE_COUNT: CounterType = 0xff;

pub type VectorizedCounter = Simd<CounterType, LANES>;
pub type CounterCondMask = <VectorizedCounter as SimdPartialEq>::Mask;
pub const MIN_COUNT: VectorizedCounter = VectorizedCounter::from_array([MIN_SINGLE_COUNT; LANES]);
pub const MAX_COUNT: VectorizedCounter = VectorizedCounter::from_array([MAX_SINGLE_COUNT; LANES]);


#[derive(Clone, Copy, Debug)]
pub struct MinimizingVectorizedCounter(VectorizedCounter);
impl From<VectorizedCounter> for MinimizingVectorizedCounter {
    fn from(val: VectorizedCounter) -> Self {
        Self(val)
    }
}
impl MinimizingVectorizedCounter {
    pub fn new() -> Self {
        Self(MAX_COUNT)
    }
    #[inline(always)]
    pub fn from_slice(slice: &[CounterType]) -> Self {
        let mut val = [MAX_SINGLE_COUNT; LANES];
        for (i, v) in slice.iter().enumerate() {
            val[i] = *v;
        }
        MinimizingVectorizedCounter(VectorizedCounter::from_array(val))
    }

    #[inline(always)]
    pub fn is_better(&self, val: VectorizedCounter) -> CounterCondMask {
        let result = val.simd_lt(self.0);
        result
    }
    #[inline(always)]
    pub fn update_min(&mut self, value: VectorizedCounter) {
        self.0 = self.0.simd_min(value);
    }
    #[inline(always)]
    pub fn get(&self) -> &VectorizedCounter {
        &self.0
    }
    #[inline(always)]
    pub fn is_uninteresting(&self) -> bool {
        self.0.simd_eq(MAX_COUNT).all()
    }
}

impl Default for MinimizingVectorizedCounter {
    fn default() -> Self {
        Self::new()
    }
}


impl Serialize for MinimizingVectorizedCounter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Vec::<CounterType>::serialize(&self.0.to_array().to_vec(), serializer)
    }
}
impl<'de> Deserialize<'de> for MinimizingVectorizedCounter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // log::info!("Deserializing MinimizingVectorizedCounter ...");
        let vec = Vec::<CounterType>::deserialize(deserializer)?;
        assert!(vec.len() == LANES);
        Ok(MinimizingVectorizedCounter::from_slice(&vec))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MaximizingVectorizedCounter(VectorizedCounter);

impl From<VectorizedCounter> for MaximizingVectorizedCounter {
    fn from(val: VectorizedCounter) -> Self {
        Self(val)
    }
}

impl MaximizingVectorizedCounter {
    #[inline(always)]
    pub fn new() -> Self {
        Self(MIN_COUNT)
    }
    #[inline(always)]
    pub fn from_slice(slice: &[CounterType]) -> MaximizingVectorizedCounter {
        let mut val = [MIN_SINGLE_COUNT; LANES];
        for (i, v) in slice.iter().enumerate() {
            val[i] = *v;
        }
        MaximizingVectorizedCounter(VectorizedCounter::from_array(val))
    }
    #[inline(always)]
    pub fn is_better(&self, val: VectorizedCounter) -> CounterCondMask {
        val.simd_gt(self.0)
    }
    #[inline(always)]
    pub fn update_max(&mut self, value: VectorizedCounter) {
        self.0 = self.0.simd_max(value);
    }
    #[inline(always)]
    pub fn get(&self) -> &VectorizedCounter {
        &self.0
    }
    #[inline(always)]
    pub fn is_uninteresting(&self) -> bool {
        self.0.simd_eq(MIN_COUNT).all()
    }
}

impl Default for MaximizingVectorizedCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl Serialize for MaximizingVectorizedCounter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Vec::<CounterType>::serialize(&self.0.to_array().to_vec(), serializer)
    }
}
impl<'de> Deserialize<'de> for MaximizingVectorizedCounter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // log::info!("Deserializing MaximizingVectorizedCounter ...");
        let vec = Vec::<CounterType>::deserialize(deserializer)?;
        assert!(vec.len() == LANES);
        Ok(MaximizingVectorizedCounter::from_slice(&vec))
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VectorizedCoverage {
    pub input_length_exponent: usize,
    pub non_zero_bitmap: BitVec,
    pub map: Vec<VectorizedCounter>,
}
impl Serialize for VectorizedCoverage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut object = serializer.serialize_struct("VectorizedCoverage", 3)?;
        object.serialize_field("input_length_exponent", &self.input_length_exponent)?;
        object.serialize_field("non_zero_bitmap", &self.non_zero_bitmap)?;
        object.serialize_field("map", &self.map.iter().map(|x| x.to_array().to_vec()).collect::<Vec<_>>())?;
        object.end()
    }
}
impl<'de> Deserialize<'de> for VectorizedCoverage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // log::info!("Deserializing VectorizedCoverage ...");
        #[derive(Deserialize)]
        struct Helper {
            input_length_exponent: usize,
            non_zero_bitmap: BitVec,
            map: Vec<Vec<CounterType>>,
        }
        let helper = Helper::deserialize(deserializer)?;

        Ok(VectorizedCoverage {
            input_length_exponent: helper.input_length_exponent,
            non_zero_bitmap: helper.non_zero_bitmap,
            map: helper.map.iter().map(|v| VectorizedCounter::from_array(v.as_slice().try_into().expect("should be able to convert to array"))).collect(),
        })
    }
}

impl VectorizedCoverage {
    pub fn from_element(input_length_exponent: usize, val: CounterType) -> Self {
        let mut array = [COUNTER_TYPE_ZERO; LANES];
        array[0] = val;
        Self {
            input_length_exponent: input_length_exponent,
            non_zero_bitmap: BitVec::from_iter(std::iter::repeat(val != 0).take(1)),
            map: vec![array.into()],
        }
    }
    pub fn count_for_branch(&self, branch_index: usize) -> CounterType {
        let vec_index = branch_index / LANES;
        self.map.get(vec_index).unwrap_or(&MIN_COUNT)[branch_index % LANES]
    }

    pub fn from_shm_slice(input_length: usize, slice: &[InstrumentationCounterType]) -> Self {
        let mut non_zero_bitmap = BitVec::repeat(false, slice.len()/LANES+1);
        let mut map = Vec::with_capacity(slice.len()/LANES+1);
        for i in 0..(slice.len() + LANES - 1) / LANES {
            let start = i * LANES;
            #[cfg(not(feature = "symcts_32bit_counters"))]
            let mut val = {
                // optimized case: 8-bit counters, no adjacent/function bits
                // produce the slice directly
                let slice = &slice[start..std::cmp::min(start + LANES, slice.len())];
                let mut array = [MIN_SINGLE_COUNT; LANES];
                for (j, &x) in slice.iter().enumerate() {
                    array[j] = if x == 0 {
                        INSTRUMENTATION_COUNTER_ZERO
                    } else {
                        let bucketed_count = get_bucketed_hitcount_inner(x as usize) as InstrumentationCounterType;
                        bucketed_count.try_into().expect("should be able to convert to CounterType")
                    };
                }
                array
            };
            #[cfg(feature = "symcts_32bit_counters")]
            let mut val = {
                let slice = [MIN_SINGLE_COUNT; LANES];
                for j in 0..LANES {
                    let x = *slice.get(start + j).unwrap_or(&0);
                    slice[j] = if x == 0 {
                        INSTRUMENTATION_COUNTER_ZERO
                    } else {
                        #[cfg(feature = "symcts_32bit_counters")]
                        let hit_count = x & !(3 << 30);
                        #[cfg(not(feature = "symcts_32bit_counters"))]
                        let hit_count = x;

                        let bucketed_count = get_bucketed_hitcount_inner(hit_count as usize) as InstrumentationCounterType;

                        let adjacent = (x & (2 << 30)) != 0;
                        let func_adjacent = (x & (1 << 30)) != 0;
                        match (bucketed_count, adjacent, func_adjacent) {
                            (0, false, true) => 1,
                            (0, true, _) => 2,
                            (x, _, _) => 2 + x,
                        }
                    }
                    .try_into().expect("should be able to convert to CounterType");
                }
            };
            let counter = VectorizedCounter::from_array(val);
            non_zero_bitmap.set(i, counter.simd_ne(MIN_COUNT).any());
            map.push(counter);
        }
        Self {
            input_length_exponent: input_length.next_power_of_two().trailing_zeros() as usize,
            non_zero_bitmap,
            map,
        }
        // let missing_for_alignment = (LANES - (slice.len() % LANES)) % LANES;
        // let bucketed_val_iter = slice
        //     .iter()
        //     .map(|&x| {
        //         if x == 0 {
        //             return 0u32;
        //         }
        //         let hit_count = x & !(3 << 30);

        //         let bucketed_count = get_bucketed_hitcount_inner(hit_count as usize) as u32;

        //         let adjacent = (x & (2 << 30)) != 0;
        //         let func_adjacent = (x & (1 << 30)) != 0;
        //         match (bucketed_count, adjacent, func_adjacent) {
        //             (0, false, true) => 1u32,
        //             (0, true, _) => 2u32,
        //             (x, _, _) => 2u32 + x,
        //         }
        //     })
        //     .chain(std::iter::repeat(0u32).take(missing_for_alignment));

        // let vectorized = bucketed_val_iter
        //     .array_chunks::<{LANES}>()
        //     .map(VectorizedCounter::from_array)
        //     .collect::<Vec<_>>();
        // Self {
        //     non_zero_bitmap: BitVec::from_iter(vectorized.iter().map(|&x| x != MIN_COUNT).collect_vec()),
        //     map: vectorized,
        // }
    }
    pub fn num_vectored_entries(&self) -> usize {
        self.map.len()
    }
    pub fn coverage_map(&self) -> &[VectorizedCounter] {
        &self.map
    }
}