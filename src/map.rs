use crate::sweep::AtomHeader;
use pathmap::zipper::{ZipperHeadOwned, ZipperValues};
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

impl<H> WeightedMap<H>
where
    H: AtomHeader,
{
    pub fn get_val(&self, path: &[u8]) -> Option<H> {
        use pathmap::zipper::{Zipper, ZipperCreation};
        // self.inner is Arc<ZipperHeadOwned<H>>. ZipperHeadOwned implements ZipperCreation.
        // read_zipper_at_path returns Result<ReadZipperTracked...>.
        match self.inner.read_zipper_at_path(path) {
            Ok(z) => z.val().cloned(),
            Err(_) => None,
        }
    }

    pub fn set_weighted_val(&self, path: &[u8], val: H) -> Result<(), ()> {
        use pathmap::zipper::{Zipper, ZipperCreation, ZipperWriting};
        // write_zipper_at_exclusive_path returns Result<WriteZipperTracked...>
        if let Ok(mut z) = self.inner.write_zipper_at_exclusive_path(path) {
            z.set_val(val);
            Ok(())
        } else {
             Err(())
        }
    }
}
