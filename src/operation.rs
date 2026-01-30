use std::sync::Arc;

use crate::sweep::AtomPosition;

/// Operation struct for transforming atoms.
///
/// # Example
/// ```ignore
/// use std::sync::Arc;
/// use weighted_atom_sweep::{Operation, AtomPosition};
///
/// fn my_transform(atom: Arc<AtomPosition>) {
///     // ... transformation logic ...
/// }
///
/// let op = Operation {
///     name: "my_operation",
///     transform: &my_transform,
/// };
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Operation {
    pub name: &'static str,
    pub transform: &'static fn(Arc<AtomPosition>),
}

impl Operation {
    /// Create a new operation with the given name and transform function.
    pub fn new(name: &'static str, transform: &'static fn(Arc<AtomPosition>)) -> Self {
        Self { name, transform }
    }
}

impl PartialEq for Operation {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && (self.transform as *const _ as usize) == (other.transform as *const _ as usize)
    }
}

/// Observer pattern trait for managing operation subscriptions.
///
/// Implementations emit debug-level traces for subscription state changes.
#[allow(dead_code)]
pub trait OperationObserver {
    /// Subscribe an operation to be executed.
    ///
    /// Emits debug-level traces about the subscription.
    fn subscribe(&mut self, operation: Operation);

    /// Unsubscribe an operation from execution.
    ///
    /// Emits debug-level traces about the unsubscription and how many operations remain.
    fn unsubscribe(&mut self, operation: Operation);
}
