use crate::sweep::{AtomHeader, AtomPosition};
use pathmap::zipper::ReadZipperTracked;
use std::error::Error;

#[derive(Debug)]
pub struct TraversalError {}
impl std::fmt::Display for TraversalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "traversal error")
    }
}
impl Error for TraversalError {}

/// Struct for traversing a structure and finding atoms.
///
/// Traversal functions should emit tracing logs at the following points:
/// - DEBUG level: When starting traversal and when atoms are found
/// - TRACE level: For detailed traversal steps and navigation decisions
///
/// # Example
/// ```ignore
/// use tracing::{instrument, debug};
/// use pathmap::zipper::ReadZipperTracked;
///
/// #[instrument(skip_all, name = "traversal.next_atom")]
/// fn my_traversal(zipper: ReadZipperTracked<MyHeader>) -> Result<AtomPosition, TraversalError> {
///     debug!("starting atom traversal");
///     // ... traversal logic ...
///     debug!(atom_path_len = atom.len(), "atom discovered");
///     Ok(atom)
/// }
///
/// let engine = TraversalEngine {
///     name: "my_traversal",
///     next_atom: &my_traversal,
/// };
/// ```
#[derive(Clone, Copy, Debug)]
pub struct TraversalEngine<H: AtomHeader> {
    pub name: &'static str,
    pub next_atom: fn(ReadZipperTracked<H>) -> Result<AtomPosition, TraversalError>,
}

impl<H: AtomHeader> TraversalEngine<H> {
    /// Create a new traversal engine with the given name and next_atom function.
    pub fn new(
        name: &'static str,
        next_atom: fn(ReadZipperTracked<H>) -> Result<AtomPosition, TraversalError>,
    ) -> Self {
        Self { name, next_atom }
    }
}

impl<H: AtomHeader> PartialEq for TraversalEngine<H> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
