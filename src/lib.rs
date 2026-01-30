//! # Weighted Atom Sweep
//!
//! A Rust library for traversing and transforming atomic data structures with comprehensive tracing support.
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
//! │   └── operations_thread  - Operation execution
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
//! Operations are defined as structs containing a name and transform function:
//!
//! ```ignore
//! use tracing::{instrument, debug};
//! use std::sync::Arc;
//! use weighted_atom_sweep::{Operation, AtomPosition};
//!
//! #[instrument(skip(atom), name = "operation.my_custom_operation")]
//! fn my_custom_transform(atom: Arc<AtomPosition>) {
//!     debug!("starting custom transformation");
//!     // Your transformation logic here
//!     debug!("transformation completed");
//! }
//!
//! let my_operation = Operation {
//!     name: "my_custom_operation",
//!     transform: &my_custom_transform,
//! };
//! ```

mod map;
mod operation;
mod sweep;
mod traversal;

pub use operation::{Operation, OperationObserver};
pub use sweep::WeightedAtomSweep;
pub use sweep::*;
pub use traversal::{TraversalEngine, TraversalError};
