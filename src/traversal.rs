use crate::sweep::{AtomHeader, AtomPosition};
use pathmap::zipper::ReadZipperTracked;
use std::error::Error;

/// Trait for traversing a structure and finding atoms.
///
/// Implementations should emit tracing logs at the following points:
/// - DEBUG level: When starting traversal and when atoms are found
/// - TRACE level: For detailed traversal steps and navigation decisions
///
/// # Example Implementation with Tracing
/// ```ignore
/// #[instrument(skip_all, name = "traversal.next_atom")]
/// fn next_atom(&self, zipper: ReadZipperTracked<H>) -> Result<AtomPosition, impl Error> {
///     debug!("starting atom traversal");
///     // ... traversal logic ...
///     debug!(atom_path_len = atom.len(), "atom discovered");
///     Ok(atom)
/// }
/// ```
pub trait TransversalEngine<H: AtomHeader> {
    /// Find the next atom starting from the given zipper position.
    ///
    /// Implementers should use `#[instrument(skip_all, name = "traversal.next_atom")]`
    /// and emit debug-level logs for important traversal milestones.
    fn next_atom(&self, zipper: ReadZipperTracked<H>) -> Result<AtomPosition, impl Error>;
}
