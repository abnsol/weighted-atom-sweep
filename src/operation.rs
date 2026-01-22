use std::sync::Arc;

use crate::sweep::{AtomHeader, AtomPosition};

/// Trait for operations that transform atoms.
///
/// Implementations should:
/// - Use `#[instrument(skip(self, zipper), name = "operation.{operation_name}")]`
/// - Emit debug-level logs for the start and completion of transformations
/// - Emit trace-level logs for detailed transformation steps
///
/// # Example Implementation with Tracing
/// ```ignore
/// use tracing::instrument;
///
/// struct MyOperation;
///
/// impl Operation<MyAtom> for MyOperation {
///     fn name(&self) -> &str { "my_operation" }
///
///     #[instrument(skip(self, zipper), name = "operation.my_operation")]
///     fn transform(&self, zipper: Arc<AtomPosition>) {
///         debug!("starting transformation");
///         // ... transformation logic ...
///         debug!("transformation complete");
///     }
/// }
/// ```
pub trait Operation<H: AtomHeader> {
    /// Get the name of this operation for logging and identification.
    fn name(&self) -> &str;

    /// Transform the given atom position.
    ///
    /// Implementers should use `#[instrument]` and emit appropriate tracing logs.
    fn transform(&self, zipper: Arc<AtomPosition>) -> ();
}

/// Observer pattern trait for managing operation subscriptions.
///
/// Implementations emit debug-level traces for subscription state changes.
pub trait OperationObserver<H, O>
where
    H: AtomHeader,
    O: Operation<H>,
{
    /// Subscribe an operation to be executed.
    ///
    /// Emits debug-level traces about the subscription.
    fn subscribe(&mut self, observer: O);

    /// Unsubscribe an operation from execution.
    ///
    /// Emits debug-level traces about the unsubscription and how many operations remain.
    fn unsubscribe(&mut self, observer: O);
}
