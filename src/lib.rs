//! # Weighted Atom Sweep
//!
//! A Rust library for traversing and transforming atomic data structures with comprehensive tracing support.
//!
//! ## Architecture
//!
//! The core abstraction is [`WeightedAtomSweep`], which orchestrates multiple
//! [`SweepProcess`] instances. Each process pairs a [`TraversalEngine`] (which
//! samples atoms from a PathMap trie) with a set of operations (implementing
//! [`TransformOp`]) that modify the subtrie at each sampled position.
//!
//! Operations receive a [`WriteZipperTracked`](pathmap::zipper::WriteZipperTracked)
//! focused at the sampled atom's position. The write zipper is scoped — operations
//! can navigate and modify everything at and below the focus, but cannot ascend
//! above it.
//!
//! ## Operation Types
//!
//! Two concrete operation types are provided:
//!
//! - [`Operation<H>`] — a stateless function pointer for simple transforms
//! - [`SExprOperation<H>`] — carries an mm2 s-expression (from mork-expr) that
//!   defines the transformation. Supports add, remove, and pattern match modes.
//!
//! Both implement the [`TransformOp<H>`] trait and can be mixed freely within a
//! single [`SweepProcess`].
//!
//! ## mm2 S-Expression Operations
//!
//! The [`SExprOperation`] type enables MORK-style s-expression operations within
//! the sweep framework. S-expressions are binary-encoded using mork-expr's [`Tag`]
//! system and stored as trie paths in the PathMap. Three modes are supported:
//!
//! - **Add**: Insert the expression as a path in the subtrie
//! - **Remove**: Delete the expression's path from the subtrie
//! - **Match**: Pattern-match the expression against the subtrie structure,
//!   with variable support (NewVar as wildcards, VarRef for co-referential bindings)
//!
//! # Tracing Instrumentation
//!
//! This crate includes comprehensive tracing support via the `tracing` crate. The instrumentation
//! is structured hierarchically to provide visibility at multiple levels:
//!
//! ## Quick Start
//!
//! To enable and view tracing output in your application, add `tracing-subscriber` as a dev dependency
//! and initialize a subscriber:
//!
//! ```ignore
//! use tracing_subscriber::fmt;
//!
//! fn main() {
//!     // Initialize the tracing subscriber with default settings
//!     fmt::init();
//!
//!     // Your code here - all tracing output will be captured
//! }
//! ```
//!
//! Or for more control over filtering:
//!
//! ```ignore
//! use tracing_subscriber::EnvFilter;
//!
//! fn main() {
//!     // Set RUST_LOG=debug (or trace) before running your application
//!     tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .init();
//! }
//! ```
//!
//! ## Tracing Hierarchy
//!
//! The crate uses a hierarchical naming convention for spans:
//!
//! ```text
//! sweep.*                    - WeightedAtomSweep operations
//! ├── sweep.new              - Initialization
//! ├── sweep.spawn            - Thread spawning and orchestration
//! │   ├── traversal_thread   - Atom traversal operations
//! │   └── operations_thread  - Operation execution (write zipper acquisition)
//! │       └── operation      - Individual operation execution
//! ├── sweep.subscribe        - Operation subscription
//! └── sweep.unsubscribe      - Operation unsubscription
//!
//! traversal.*                - TransversalEngine operations
//! └── traversal.next_atom    - Finding the next atom to process
//!
//! operation.*                - Operation trait methods
//! └── operation.*            - Individual operation implementations
//!
//! map.*                      - WeightedMap operations
//! ```
//!
//! ## Log Levels
//!
//! The crate follows these conventions for log levels:
//!
//! - **ERROR**: Critical failures that prevent operation continuation
//! - **WARN**: Recoverable issues or deprecated patterns
//! - **INFO**: Major milestones and state changes
//! - **DEBUG**: Function entry/exit, important operations, state transitions
//! - **TRACE**: Detailed operation steps, value inspections, minor decisions
//!
//! ## Example with Filtering
//!
//! ```ignore
//! use tracing_subscriber::fmt;
//! use tracing_subscriber::filter::EnvFilter;
//!
//! fn main() {
//!     // Show only sweep operations at debug level, operation details at trace level
//!     let filter = EnvFilter::try_from_default_env()
//!         .unwrap_or_else(|_| EnvFilter::new("sweep=debug,operation=trace"));
//!
//!     fmt()
//!         .with_env_filter(filter)
//!         .init();
//! }
//! ```
//!
//! ## Creating Custom Operations
//!
//! Operations implement the [`TransformOp`] trait:
//!
//! ```ignore
//! use tracing::{instrument, debug};
//! use pathmap::zipper::WriteZipperTracked;
//! use weighted_atom_sweep::{TransformOp, AtomHeader};
//!
//! struct MyCustomOp;
//!
//! impl<H: AtomHeader> TransformOp<H> for MyCustomOp {
//!     fn name(&self) -> &str { "my_custom_operation" }
//!     fn apply(&self, wz: &mut WriteZipperTracked<H>) {
//!         debug!("starting custom transformation");
//!         // Navigate and modify the subtrie via wz
//!         debug!("transformation completed");
//!     }
//! }
//! ```
//!
//! Or use the simple function pointer form:
//!
//! ```ignore
//! use weighted_atom_sweep::{Operation, AtomHeader};
//! use pathmap::zipper::WriteZipperTracked;
//!
//! fn my_transform(wz: &mut WriteZipperTracked<MyHeader>) {
//!     // ... transformation logic ...
//! }
//!
//! let op = Operation::new("my_operation", my_transform);
//! ```

mod map;
mod operation;
pub mod sexpr_operation;
mod sweep;
mod traversal;

pub use operation::{Operation, OperationObserver, TransformOp};
pub use sexpr_operation::{SExprMode, SExprOperation};
pub use sweep::WeightedAtomSweep;
pub use sweep::*;
pub use traversal::{TraversalEngine, TraversalError};
