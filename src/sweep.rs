use crate::map::WeightedMap;
use crate::operation::{Operation, OperationObserver};
use crate::traversal::TraversalEngine;
use pathmap::PathMap;
use pathmap::zipper::{ZipperCreation, ZipperHeadOwned};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use tracing::{Level, debug, instrument, span, trace};

pub type AtomPosition = Vec<u8>;

pub trait AtomHeader: std::fmt::Debug + Clone + Send + Sync + Unpin + 'static {}

#[derive(Default)]
pub struct WeightedAtomSweepSettings {}

/// Represents a single traversal engine with its subscribed operations.
///
/// Each process spawns 2 threads when the sweep is started:
/// - A traversal thread that continuously samples atoms using the engine
/// - An operations thread that applies subscribed operations to sampled atoms
pub struct SweepProcess<H>
where
    H: AtomHeader,
{
    engine: TraversalEngine<H>,
    operations: Vec<Operation>,
}

impl<H> SweepProcess<H>
where
    H: AtomHeader,
{
    /// Create a new traversal process with the given engine and no operations.
    pub fn new(engine: TraversalEngine<H>) -> Self {
        debug!("creating new TraversalProcess");
        Self {
            engine,
            operations: Vec::new(),
        }
    }

    /// Get the number of operations subscribed to this process.
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }
}

impl<H> OperationObserver for SweepProcess<H>
where
    H: AtomHeader,
{
    #[instrument(skip_all, name = "process.subscribe", fields(op_name = operation.name))]
    fn subscribe(&mut self, operation: Operation) {
        let total_operations = self.operations.len() + 1;
        debug!(
            operation_name = operation.name,
            total_operations, "subscribing operation to process"
        );
        self.operations.push(operation);
        trace!("operation subscribed successfully");
    }

    #[instrument(skip_all, name = "process.unsubscribe", fields(op_name = operation.name))]
    fn unsubscribe(&mut self, operation: Operation) {
        let initial_count = self.operations.len();
        debug!(
            operation_name = operation.name,
            initial_count, "unsubscribing operation from process"
        );
        self.operations.retain(|op| op != &operation);
        let final_count = self.operations.len();
        let removed = initial_count - final_count;
        debug!(removed, final_count, "operation unsubscribe complete");
    }
}

pub struct SweepController<H: AtomHeader> {
    pub map: Arc<ZipperHeadOwned<H>>,
    handles: Vec<JoinHandle<()>>,
    shutdown_signal: Arc<AtomicBool>,
}

impl<H: AtomHeader> SweepController<H> {
    /// Wait for sweep to complete naturally (when threads finish)
    pub fn wait(mut self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("waiting for sweep completion");

        for handle in self.handles.drain(..) {
            handle.join().map_err(|_| "thread panicked")?;
        }

        debug!("sweep completed");
        Ok(())
    }

    /// Signal threads to shutdown and wait for them to terminate
    pub fn shutdown(mut self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("initiating sweep shutdown");
        self.shutdown_signal.store(true, Ordering::SeqCst);

        for handle in self.handles.drain(..) {
            handle
                .join()
                .map_err(|_| "thread panicked during shutdown")?;
        }

        debug!("sweep shutdown complete");
        Ok(())
    }

    /// Get a reference to the map for external access
    pub fn map_ref(&self) -> &Arc<ZipperHeadOwned<H>> {
        &self.map
    }

    /// Get the number of threads managed by this controller.
    /// This will be 2*N where N is the number of processes.
    pub fn thread_count(&self) -> usize {
        self.handles.len()
    }

    /// Get the number of processes (thread pairs) in this sweep.
    pub fn process_count(&self) -> usize {
        self.handles.len() / 2
    }
}

#[allow(dead_code)]
pub struct WeightedAtomSweep<H>
where
    H: AtomHeader,
{
    processes: Vec<SweepProcess<H>>,
    settings: WeightedAtomSweepSettings,
    map: WeightedMap<H>,
}

impl<H> WeightedAtomSweep<H>
where
    H: AtomHeader,
{
    #[instrument(skip_all, name = "sweep.new")]
    pub fn new(settings: WeightedAtomSweepSettings) -> Self {
        debug!("initializing WeightedAtomSweep");
        trace!("creating new PathMap and initializing WeightedMap");

        let result = Self {
            processes: Vec::new(),
            settings,
            map: WeightedMap {
                inner: Arc::new(PathMap::<H>::new().into_zipper_head([])),
            },
        };

        debug!("WeightedAtomSweep initialization complete");
        result
    }

    /// Add a traversal engine to the sweep and return a mutable reference
    /// to configure it (subscribe operations).
    ///
    /// # Example
    /// ```ignore
    /// let mut sweep = WeightedAtomSweep::new(settings);
    /// let process = sweep.add_engine(importance_engine);
    /// process.subscribe(importance_op);
    /// ```
    #[instrument(skip_all, name = "sweep.add_engine")]
    pub fn add_engine(&mut self, engine: TraversalEngine<H>) -> &mut SweepProcess<H> {
        debug!("adding new traversal engine to sweep");
        let process = SweepProcess::new(engine);
        self.processes.push(process);
        let process_count = self.processes.len();
        debug!(process_count, "engine added successfully");
        self.processes.last_mut().unwrap()
    }

    /// Get the number of processes (engines) in this sweep.
    pub fn process_count(&self) -> usize {
        self.processes.len()
    }

    // TODO: map can be limited to a subset of the map
    #[instrument(skip_all, name = "sweep.spawn")]
    pub fn spawn(self) -> SweepController<H> {
        let process_count = self.processes.len();
        debug!(process_count, "spawning WeightedAtomSweep threads");

        if process_count == 0 {
            debug!("warning: no processes added, sweep will do nothing");
        }

        let map = self.map.inner.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();

        // Spawn thread pairs for each process
        for (process_idx, process) in self.processes.into_iter().enumerate() {
            let engine = process.engine.clone();
            let operations = process.operations;
            let operation_count = operations.len();

            // Clone shared resources for this process
            let map_for_traversal = self.map.inner.clone();
            let shutdown_traversal = shutdown.clone();
            let shutdown_operations = shutdown.clone();

            // Create channel for this process
            let (atom_sender, atom_receiver) = mpsc::channel::<AtomPosition>();

            // Spawn traversal thread for this process
            let traversal_handle = std::thread::spawn(move || {
                let traversal_span = span!(Level::DEBUG, "traversal_thread", process_idx);
                let _enter = traversal_span.enter();

                debug!(
                    process_idx,
                    "traversal thread started - entering sampling loop"
                );

                loop {
                    // Check for shutdown signal
                    if shutdown_traversal.load(Ordering::Relaxed) {
                        debug!(
                            process_idx,
                            "shutdown signal received, exiting traversal loop"
                        );
                        break;
                    }

                    // Get access to a read zipper at root for sampling
                    match (*map_for_traversal).read_zipper_at_borrowed_path(&[]) {
                        Ok(traverse_zp) => {
                            trace!(process_idx, "acquired read zipper for sampling");
                            match (engine.next_atom)(traverse_zp) {
                                Ok(atom_path) => {
                                    debug!(
                                        process_idx,
                                        atom_path_len = atom_path.len(),
                                        "atom sampled via traversal"
                                    );
                                    if atom_sender.send(atom_path).is_err() {
                                        debug!(
                                            process_idx,
                                            "operations thread terminated - stopping traversal"
                                        );
                                        break; // TODO: consider continuing traversal on failure
                                    }
                                }
                                Err(e) => {
                                    // Log and continue (resilient mode)
                                    trace!(process_idx, "error during atom traversal: {:?}", e);
                                }
                            }
                        }
                        Err(e) => {
                            trace!(process_idx, "failed to acquire read zipper: {:?}", e);
                        }
                    }
                }

                debug!(process_idx, "traversal thread completed");
                drop(atom_sender); // Signal operations thread
            });

            handles.push(traversal_handle);

            // Spawn operations thread for this process
            let operations_handle = std::thread::spawn(move || {
                let operations_span = span!(
                    Level::DEBUG,
                    "operations_thread",
                    process_idx,
                    operation_count
                );
                let _enter = operations_span.enter();

                debug!(
                    process_idx,
                    operation_count, "operations thread started - entering processing loop"
                );

                loop {
                    match atom_receiver.recv() {
                        Ok(atom_path) => {
                            let atom = Arc::new(atom_path);
                            debug!(
                                process_idx,
                                atom_len = atom.len(),
                                operation_count,
                                "processing atom with operations"
                            );

                            for (idx, op) in operations.iter().enumerate() {
                                let op_span = span!(
                                    Level::TRACE,
                                    "operation",
                                    process_idx,
                                    operation_idx = idx,
                                    name = op.name
                                );
                                let _op_enter = op_span.enter();

                                trace!(process_idx, "executing operation");

                                // Catch panics to prevent thread death
                                let result =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        (op.transform)(atom.clone());
                                    }));

                                if let Err(e) = result {
                                    trace!(process_idx, "operation panicked: {:?}", e);
                                } else {
                                    trace!(process_idx, "operation completed");
                                }
                            }

                            debug!(process_idx, "all operations completed for atom");
                        }
                        Err(_) => {
                            debug!(
                                process_idx,
                                "traversal complete - channel closed, no more atoms"
                            );
                            break;
                        }
                    }
                }

                debug!(process_idx, "operations thread completed");
                shutdown_operations.store(true, Ordering::Relaxed);
            });

            handles.push(operations_handle);

            debug!(process_idx, "spawned thread pair for process");
        }

        debug!(
            total_threads = handles.len(),
            "spawn operation complete, returning controller"
        );

        SweepController {
            map,
            handles,
            shutdown_signal: shutdown,
        }
    }
}
