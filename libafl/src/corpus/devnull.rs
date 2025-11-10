//! The null corpus does not store any [`Testcase`]s.
use core::{cell::RefCell, marker::PhantomData};

use serde::{Deserialize, Serialize};

use crate::{
    corpus::{Corpus, CorpusId, Testcase},
    inputs::{Input, UsesInput},
    Error,
};

/// A corpus which does not store any [`Testcase`]s.
#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(bound = "I: serde::de::DeserializeOwned")]
pub struct DevNullCorpus<I> {
    num_inputs: usize,
    empty: Option<CorpusId>,
    phantom: PhantomData<I>,
}

impl<I> UsesInput for DevNullCorpus<I>
where
    I: Input,
{
    type Input = I;
}

impl<I> Corpus for DevNullCorpus<I>
where
    I: Input,
{
    /// Returns the number of elements
    #[inline]
    fn count(&self) -> usize {
        return self.num_inputs;
    }

    /// Add an entry to the corpus and return its index
    #[inline]
    fn add(&mut self, _testcase: Testcase<I>) -> Result<CorpusId, Error> {
        log::info!("Adding testcase to DevNullCorpus: {} total added so far", self.num_inputs + 1);
        self.num_inputs += 1;
        return Ok(CorpusId::from(self.num_inputs));
    }

    /// Replaces the testcase at the given idx
    #[inline]
    fn replace(&mut self, _idx: CorpusId, _testcase: Testcase<I>) -> Result<Testcase<I>, Error> {
        Err(Error::unsupported("Unsupported by DevNullCorpus"))
    }

    /// Removes an entry from the corpus, returning it if it was present.
    #[inline]
    fn remove(&mut self, _idx: CorpusId) -> Result<Testcase<I>, Error> {
        Err(Error::unsupported("Unsupported by DevNullCorpus"))
    }

    /// Get by id
    #[inline]
    fn get(&self, _idx: CorpusId) -> Result<&RefCell<Testcase<I>>, Error> {
        Err(Error::unsupported("Unsupported by DevNullCorpus"))
    }

    /// Current testcase scheduled
    #[inline]
    fn current(&self) -> &Option<CorpusId> {
        &self.empty
    }

    /// Current testcase scheduled (mutable)
    #[inline]
    fn current_mut(&mut self) -> &mut Option<CorpusId> {
        &mut self.empty
    }

    #[inline]
    fn next(&self, _idx: CorpusId) -> Option<CorpusId> {
        None
    }

    #[inline]
    fn prev(&self, _idx: CorpusId) -> Option<CorpusId> {
        None
    }

    #[inline]
    fn first(&self) -> Option<CorpusId> {
        None
    }

    #[inline]
    fn last(&self) -> Option<CorpusId> {
        None
    }

    #[inline]
    fn nth(&self, _nth: usize) -> CorpusId {
        CorpusId::from(0_usize)
    }

    #[inline]
    fn load_input_into(&self, _testcase: &mut Testcase<Self::Input>) -> Result<(), Error> {
        Err(Error::unsupported("Unsupported by DevNullCorpus"))
    }

    #[inline]
    fn store_input_from(&self, _testcase: &Testcase<Self::Input>) -> Result<(), Error> {
        Err(Error::unsupported("Unsupported by DevNullCorpus"))
    }
}

impl<I> DevNullCorpus<I>
where
    I: Input,
{
    /// Creates a new [`DevNullCorpus`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            num_inputs: 0,
            empty: None,
            phantom: PhantomData {},
        }
    }
}
