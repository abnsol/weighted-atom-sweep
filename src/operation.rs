use crate::sweep::AtomHeader;
use pathmap::zipper::WriteZipperTracked;

/// Trait for operations that transform a submap of the PathMap trie.
///
/// Operations receive a mutable reference to a [`WriteZipperTracked`] focused at the
/// atom's position in the trie, plus the raw `atom_path` bytes returned by the
/// traversal engine. The write zipper is scoped to the subtrie at that position —
/// the operation can read and modify everything at and below the focus, but cannot
/// ascend above it.
///
/// The `atom_path` parameter provides contextual information about which atom was
/// sampled. The zipper is already focused at this path, so operations do not need
/// to manually descend to it — it serves as metadata (e.g., for logging or for
/// [`SExprOperation`](crate::sexpr_operation::SExprOperation) which uses it to
/// understand its execution context).
///
/// Two concrete implementations are provided:
/// - [`Operation<H>`] — a stateless function pointer for simple transforms
/// - [`SExprOperation<H>`](crate::sexpr_operation::SExprOperation) — an mm2 exec
///   operation that pattern-matches and instantiates templates within the subtrie
///
/// # Example
/// ```ignore
/// use weighted_atom_sweep::{TransformOp, AtomHeader};
/// use pathmap::zipper::WriteZipperTracked;
///
/// struct MyOp;
///
/// impl<H: AtomHeader> TransformOp<H> for MyOp {
///     fn name(&self) -> &str { "my_op" }
///     fn apply(&self, wz: &mut WriteZipperTracked<H>, atom_path: &[u8]) {
///         // ... modify the subtrie via wz ...
///     }
///  }
/// ```
pub trait TransformOp<H: AtomHeader>: Send + Sync {
    /// Returns the name of this operation, used for tracing and identification.
    fn name(&self) -> &str;

    /// Apply the operation to the subtrie accessible via the write zipper.
    ///
    /// # Arguments
    /// * `wz` — Write zipper focused at the atom's position in the PathMap trie.
    /// * `atom_path` — The raw byte path returned by the traversal engine. The
    ///   zipper is already focused at this path; this parameter provides context.
    fn apply(&self, wz: &mut WriteZipperTracked<H>, atom_path: &[u8]);
}

/// A stateless operation defined by a function pointer.
///
/// This is the simplest form of operation — a named function that receives a
/// [`WriteZipperTracked`] focused at the atom's position and the atom path bytes.
/// The function can read and modify the subtrie but cannot ascend above the focus.
///
/// # Example
/// ```ignore
/// use weighted_atom_sweep::{Operation, AtomHeader};
/// use pathmap::zipper::{WriteZipperTracked, ZipperValues};
///
/// #[derive(Debug, Clone, Default)]
/// struct Header { value: u64 }
/// impl AtomHeader for Header {}
///
/// fn inspect(wz: &mut WriteZipperTracked<Header>, _atom_path: &[u8]) {
///     if let Some(val) = wz.val() {
///         tracing::debug!(value = val.value, "inspecting atom");
///     }
/// }
///
/// let op = Operation::<Header>::new("inspect", inspect);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Operation<H: AtomHeader> {
    pub name: &'static str,
    pub transform: fn(&mut WriteZipperTracked<H>, &[u8]),
}

impl<H: AtomHeader> Operation<H> {
    /// Create a new operation with the given name and transform function.
    pub fn new(name: &'static str, transform: fn(&mut WriteZipperTracked<H>, &[u8])) -> Self {
        Self { name, transform }
    }
}

impl<H: AtomHeader> PartialEq for Operation<H> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && (self.transform as usize) == (other.transform as usize)
    }
}

impl<H: AtomHeader> TransformOp<H> for Operation<H> {
    fn name(&self) -> &str {
        self.name
    }

    fn apply(&self, wz: &mut WriteZipperTracked<H>, atom_path: &[u8]) {
        (self.transform)(wz, atom_path);
    }
}

/// Observer pattern trait for managing operation subscriptions on a sweep process.
///
/// Implementations emit debug-level traces for subscription state changes.
pub trait OperationObserver<H: AtomHeader> {
    /// Subscribe an operation to be executed during the sweep.
    ///
    /// Accepts any type implementing [`TransformOp<H>`], including both
    /// simple [`Operation<H>`] and [`SExprOperation<H>`](crate::sexpr_operation::SExprOperation).
    fn subscribe(&mut self, operation: impl TransformOp<H> + 'static);

    /// Unsubscribe an operation by name. All operations with the matching name
    /// will be removed.
    fn unsubscribe_by_name(&mut self, name: &str);
}
