use crate::sweep::AtomHeader;
use pathmap::zipper::ZipperHeadOwned;
use std::{ops::Deref, sync::Arc};

/// A thread-safe wrapper around PathMap's ZipperHeadOwned for managing weighted atoms.
///
/// # Tracing
/// This struct serves as a container for the atom map used throughout the sweep process.
/// Operations using this map should emit traces at the following levels:
/// - DEBUG: For significant structural operations (initialization, major updates)
/// - TRACE: For detailed navigation and zipper operations (path lookups, position changes)
///
/// The actual tracing is handled by code using WeightedMap, particularly in the
/// WeightedAtomSweep module where zippers are accessed and atoms are processed.
pub struct WeightedMap<H: AtomHeader> {
    pub inner: Arc<ZipperHeadOwned<H>>,
}

impl<H> Deref for WeightedMap<H>
where
    H: AtomHeader,
{
    type Target = ZipperHeadOwned<H>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
