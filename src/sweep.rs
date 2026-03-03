use crate::map::WeightedMap;
use crate::operation::{OperationObserver, TransformOp};
use crate::traversal::TraversalEngine;
use pathmap::zipper::{ZipperCreation, ZipperHeadOwned, ZipperMoving};
use pathmap::PathMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use tracing::{debug, instrument, span, trace, Level};

pub type AtomPosition = Vec<u8>;

pub trait AtomHeader: std::fmt::Debug + Clone + Send + Sync + Unpin + 'static {}

#[derive(Default)]
pub struct WeightedAtomSweepSettings {}

/// Represents a single traversal engine with its subscribed operations.
///
/// Each process spawns 2 threads when the sweep is started:
/// - A traversal thread that continuously samples atoms using the engine
/// - An operations thread that acquires write zippers at sampled positions
///   and applies subscribed operations to the focused subtrie
pub struct SweepProcess<H>
where
    H: AtomHeader,
{
    engine: TraversalEngine<H>,
    operations: Vec<Box<dyn TransformOp<H>>>,
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

impl<H> OperationObserver<H> for SweepProcess<H>
where
    H: AtomHeader,
{
    #[instrument(skip_all, name = "process.subscribe")]
    fn subscribe(&mut self, operation: impl TransformOp<H> + 'static) {
        let name = operation.name().to_string();
        let total_operations = self.operations.len() + 1;
        debug!(
            operation_name = %name,
            total_operations, "subscribing operation to process"
        );
        self.operations.push(Box::new(operation));
        trace!("operation subscribed successfully");
    }

    #[instrument(skip_all, name = "process.unsubscribe_by_name")]
    fn unsubscribe_by_name(&mut self, name: &str) {
        let initial_count = self.operations.len();
        debug!(
            operation_name = name,
            initial_count, "unsubscribing operation from process"
        );
        self.operations.retain(|op| op.name() != name);
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
    pub map: WeightedMap<H>,
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
    /// process.subscribe(my_operation);
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
            let map_for_operations = self.map.inner.clone();
            let shutdown_traversal = shutdown.clone();
            let shutdown_operations = shutdown.clone();
            let pause_flag = Arc::new(AtomicBool::new(false));
            let pause_for_traversal = pause_flag.clone();
            let pause_for_operations = pause_flag.clone();

            // Create channel for this process
            let (atom_sender, atom_receiver) = mpsc::channel::<AtomPosition>();

            // Spawn traversal thread for this process
            //
            // The traversal thread creates a ReadZipperTracked at root, uses the
            // engine to sample an atom, then DROPS the read zipper before sending
            // the AtomPosition through the channel. This ensures no read zipper is
            // held while the operations thread acquires a write zipper, avoiding
            // zipper conflicts.
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

                    // Check pause flag - if operations has a conflict, wait before sampling
                    while pause_for_traversal.load(Ordering::Relaxed) {
                        if shutdown_traversal.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }

                    // Check shutdown again after pause
                    if shutdown_traversal.load(Ordering::Relaxed) {
                        break;
                    }

                    // Get access to a read zipper at root for sampling.
                    // The read zipper is created and consumed within this match block,
                    // ensuring it is dropped before atom_sender.send() is called.
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
                                        break;
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
            //
            // For each received AtomPosition, the operations thread acquires a
            // WriteZipperTracked focused at that path via write_zipper_at_exclusive_path.
            // This write zipper is scoped: the operation can navigate and modify the
            // subtrie at and below the focus, but cannot ascend above it.
            //
            // After all operations complete, the write zipper is cleaned up via
            // cleanup_write_zipper to prune any dangling paths created by the
            // exclusive path mechanism.
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

                let mut buffer: Vec<AtomPosition> = Vec::new();

                loop {
                    // Try to receive new atom from traversal (non-blocking)
                    if let Ok(atom_path) = atom_receiver.try_recv() {
                        debug!(
                            process_idx,
                            atom_path_len = atom_path.len(),
                            operation_count,
                            "received atom from traversal, pushing to buffer"
                        );
                        buffer.push(atom_path);
                    }

                    // Process buffer in FIFO order
                    let mut made_progress = false;
                    let mut i = 0;
                    while i < buffer.len() {
                        let atom_path = &buffer[i];

                        debug!(
                            process_idx,
                            atom_path_len = atom_path.len(),
                            operation_count,
                            "processing atom from buffer"
                        );

                        // Acquire a write zipper at the atom's position.
                        // This gives exclusive write access to the subtrie at
                        // and below atom_path. The zipper's root IS atom_path,
                        // so ascend() returns false at this boundary.
                        match map_for_operations.write_zipper_at_exclusive_path(&atom_path[..]) {
                            Ok(mut wz) => {
                                for (idx, op) in operations.iter().enumerate() {
                                    let op_span = span!(
                                        Level::TRACE,
                                        "operation",
                                        process_idx,
                                        operation_idx = idx,
                                        name = op.name()
                                    );
                                    let _op_enter = op_span.enter();

                                    trace!(process_idx, "executing operation");

                                    // Catch panics to prevent thread death
                                    let result = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| {
                                            op.apply(&mut wz, atom_path);
                                        }),
                                    );

                                    if let Err(e) = result {
                                        trace!(process_idx, "operation panicked: {:?}", e);
                                    } else {
                                        trace!(process_idx, "operation completed");
                                    }

                                    // Reset the write zipper to the focus root
                                    // between operations so each starts at the
                                    // same position
                                    wz.reset();
                                }

                                // Cleanup the write zipper: drops it and prunes
                                // any dangling path nodes created by the exclusive
                                // path mechanism
                                map_for_operations.cleanup_write_zipper(wz);

                                debug!(process_idx, "all operations completed for atom");
                                buffer.remove(i);
                                made_progress = true;
                            }
                            Err(conflict) => {
                                // Another process may hold a conflicting zipper.
                                // Set pause flag to slow down traversal, keep in buffer.
                                trace!(
                                    process_idx,
                                    "write zipper conflict for atom, pausing traversal: {:?}",
                                    conflict
                                );
                                pause_for_operations.store(true, Ordering::Relaxed);
                                i += 1;
                            }
                        }
                    }

                    // Clear pause flag only if buffer is empty
                    if buffer.is_empty() {
                        pause_for_operations.store(false, Ordering::Relaxed);
                    }

                    // If we couldn't make progress (all conflicts), sleep before retry
                    if !made_progress && !buffer.is_empty() {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }

                    // Check if traversal has ended and buffer is empty
                    if buffer.is_empty() {
                        // Use try_recv to check if channel is disconnected
                        match atom_receiver.try_recv() {
                            Ok(atom_path) => {
                                buffer.push(atom_path);
                            }
                            Err(mpsc::TryRecvError::Disconnected) => {
                                debug!(
                                    process_idx,
                                    "traversal complete - channel closed, buffer empty, exiting"
                                );
                                break;
                            }
                            Err(mpsc::TryRecvError::Empty) => {
                                // Channel still open but no new messages, continue
                            }
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
