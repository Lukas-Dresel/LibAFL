//! The cached ondisk corpus stores testcases to disk keeping a part of them in memory.

use core::cell::RefCell;
use libafl::{corpus::{CorpusId, CorpusIdManager, InMemoryCorpus}, state::HasMetadata};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, collections::HashMap};

use libafl::{
    corpus::{
        ondisk::OnDiskMetadataFormat,
        cached::CachedOnDiskCorpus,
        Corpus, Testcase,
    },
    inputs::Input,
    Error,
};

use crate::coverage::CoveragePoint;

/// A corpus that adds testcases that can be either permanently stored or only temporarily in memory. Concretely,
/// inputs that are the shortest input for a certain coverage identifier are stored permanently, as they change infrequently.
/// By contrast, we also always update the last input to have been seen executing a specific coverage point. This changes frequently,
/// and is stored in-memory only and remove once the next input is executed
#[cfg(feature = "std")]
#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(bound = "I: serde::de::DeserializeOwned")]
pub struct SyMCTSCorpus<I>
where
    I: Input,
{
    shortest_inputs_corpus: CachedOnDiskCorpus<I>,
    latest_inputs_corpus: InMemoryCorpus<I>,
    shortest_seen: HashMap<CoveragePoint, (usize, CorpusId)>,
    outer_to_inner: HashMap<CorpusId, InnerCorpusId>,
    current: Option<CorpusId>,
    id_manager: CorpusIdManager,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(self) enum InnerCorpusId {
    Persistent(CorpusId),
    Ephemeral(CorpusId),
}

impl<I> Corpus<I> for SyMCTSCorpus<I>
where
    I: Input,
{
    /// Returns the number of elements
    #[inline]
    fn count(&self) -> usize {
        assert!(
            self.shortest_inputs_corpus.count() + self.latest_inputs_corpus.count()
                == self.outer_to_inner.len()
        );
        self.outer_to_inner.len()
    }

    /// Add an entry to the corpus and return its index
    #[inline]
    fn add(&mut self, testcase: Testcase<I>) -> Result<CorpusId, Error> {
        let tc_meta = testcase
            .metadata()
            .get::<SyMCTSTestcaseMetadata>()
            .unwrap();

        let mut shortest_for_cov_point = None;
        for cov_point in tc_meta.coverage_summary.hit.iter() {
            // TODO refactor this into iterator crap, this loop is terrible
            if !self.shortest_seen.contains_key(cov_point) {
                shortest_for_cov_point = Some(cov_point.clone());
                break;
            }

            let (cur_len, _cur_idx) = self.shortest_seen.get(cov_point).unwrap();
            if *cur_len > tc_meta.input_size {
                shortest_for_cov_point = Some(cov_point.clone());
                break;
            }
        }
        let inner_id = if let Some(cov_point) = shortest_for_cov_point {
            let inp_len = tc_meta.input_size;
            let inner_id = self.shortest_inputs_corpus.add(testcase)?;
            self.shortest_seen.insert(cov_point, (inp_len, inner_id));
            InnerCorpusId::Persistent(inner_id)
        } else {
            InnerCorpusId::Ephemeral(self.latest_inputs_corpus.add(testcase)?)
        };
        let new_corpus_id = self.id_manager.provide_next()?;
        self.outer_to_inner.insert(new_corpus_id, inner_id);
        Ok(new_corpus_id)
    }

    /// Replaces the testcase at the given idx
    #[inline]
    fn replace(&mut self, _idx: CorpusId, _testcase: Testcase<I>) -> Result<(), Error> {
        // TODO finish
        todo!();
    }

    /// Removes an entry from the corpus, returning it if it was present.
    #[inline]
    fn remove(&mut self, _idx: CorpusId) -> Result<Option<Testcase<I>>, Error> {
        todo!();
    }

    /// Get by id
    #[inline]
    fn get(&self, id: CorpusId) -> Result<&RefCell<Testcase<I>>, Error> {
        let inner_id = self.outer_to_inner.get(&id).unwrap().clone();

        match inner_id {
            InnerCorpusId::Persistent(id) => self.shortest_inputs_corpus.get(id),
            InnerCorpusId::Ephemeral(id) => self.latest_inputs_corpus.get(id),
        }
    }

    /// Current testcase scheduled
    #[inline]
    fn current(&self) -> &Option<CorpusId> {
        &self.current
    }

    /// Current testcase scheduled (mutable)
    #[inline]
    fn current_mut(&mut self) -> &mut Option<CorpusId> {
        &mut self.current
    }

    fn id_manager(&self) -> &CorpusIdManager {
        &self.id_manager
    }
}

impl<I> SyMCTSCorpus<I>
where
    I: Input,
{
    /// Creates the [`CachedOnDiskCorpus`].
    pub fn new(persistent_dir_path: PathBuf) -> Result<Self, Error> {
        Ok(Self {
            shortest_inputs_corpus: CachedOnDiskCorpus::new_save_meta(
                persistent_dir_path,
                Some(OnDiskMetadataFormat::Postcard),
                1024,
            )?,
            latest_inputs_corpus: InMemoryCorpus::new(),
            shortest_seen: HashMap::new(),
            outer_to_inner: HashMap::new(),
            current: None,
            id_manager: CorpusIdManager::new(),
        })
    }

    /// Creates the [`CachedOnDiskCorpus`] specifying the type of `Metadata` to be saved to disk.
    pub fn new_save_meta(
        persistent_dir_path: PathBuf,
        meta_format: Option<OnDiskMetadataFormat>,
        cache_max_len: usize,
    ) -> Result<Self, Error> {
        Ok(Self {
            shortest_inputs_corpus: CachedOnDiskCorpus::new_save_meta(
                persistent_dir_path,
                meta_format,
                cache_max_len,
            )?,
            latest_inputs_corpus: InMemoryCorpus::new(),
            shortest_seen: HashMap::new(),
            outer_to_inner: HashMap::new(),
            current: None,
            id_manager: CorpusIdManager::new(),
        })
    }
}

/// ``CachedOnDiskCorpus`` Python bindings
#[cfg(feature = "python")]
pub mod pybind {
    use std::path::PathBuf;

    use lib::corpus::CachedOnDiskCorpus;
    use lib::inputs::BytesInput;
    use pyo3::prelude::*;
    use serde::{Deserialize, Serialize};

    #[pyclass(unsendable, name = "CachedOnDiskCorpus")]
    #[derive(Serialize, Deserialize, Debug, Clone)]
    /// Python class for CachedOnDiskCorpus
    pub struct PythonCachedOnDiskCorpus {
        /// Rust wrapped CachedOnDiskCorpus object
        pub cached_on_disk_corpus: CachedOnDiskCorpus<BytesInput>,
    }

    #[pymethods]
    impl PythonCachedOnDiskCorpus {
        #[new]
        fn new(path: String, cache_max_len: usize) -> Self {
            Self {
                cached_on_disk_corpus: CachedOnDiskCorpus::new(PathBuf::from(path), cache_max_len)
                    .unwrap(),
            }
        }
    }
    /// Register the classes to the python module
    pub fn register(_py: Python, m: &PyModule) -> PyResult<()> {
        m.add_class::<PythonCachedOnDiskCorpus>()?;
        Ok(())
    }
}
