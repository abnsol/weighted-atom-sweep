use crate::map::WeightedMap;
use crate::operation::{Operation, OperationObserver};
use crate::traversal::TransversalEngine;
use pathmap::PathMap;
use pathmap::zipper::{ZipperCreation, ZipperHeadOwned};
use std::sync::{Arc, mpsc, atomic::{AtomicBool, Ordering}};
use std::thread::JoinHandle;
use tracing::{debug, trace, instrument, span, Level};

pub type AtomPosition = Vec<u8>;

pub trait AtomHeader: std::fmt::Debug + Clone + Send + Sync + Unpin + 'static {}
pub trait KernelOperation<H: AtomHeader>:
    Operation<H> + Send + Sync + Clone + std::fmt::Debug + PartialEq + 'static
{
}

pub trait SweepTransversalEngine<H: AtomHeader>:
    for<'a> TransversalEngine<H> + Send + Sync + Clone + std::fmt::Debug + 'static
{
}

pub struct WeightedAtomSweepSettings {}

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
            handle.join().map_err(|_| "thread panicked during shutdown")?;
        }

        debug!("sweep shutdown complete");
        Ok(())
    }

    /// Get a reference to the map for external access
    pub fn map_ref(&self) -> &Arc<ZipperHeadOwned<H>> {
        &self.map
    }
}

pub struct WeightedAtomSweep<T, O, H>
where
    T: SweepTransversalEngine<H>,
    O: KernelOperation<H>,
    H: AtomHeader,
{
    // pub reciever: mpsc::Receiver<T::Atom>,
    pub traversal: Arc<T>,
    pub operations: Vec<O>,
    pub settings: WeightedAtomSweepSettings,
    pub map: WeightedMap<H>,
}

impl<T, O, H> WeightedAtomSweep<T, O, H>
where
    T: SweepTransversalEngine<H>,
    O: KernelOperation<H>,
    H: AtomHeader,
{
    #[instrument(skip_all, name = "sweep.new")]
    pub fn new(traversal: T, operations: Vec<O>, settings: WeightedAtomSweepSettings) -> Self {
        let operation_count = operations.len();
        debug!(operation_count, "initializing WeightedAtomSweep");
        trace!("creating new PathMap and initializing WeightedMap");

        let result = Self {
            traversal: Arc::new(traversal),
            operations: operations,
            settings,
            map: WeightedMap {
                inner: Arc::new(PathMap::<H>::new().into_zipper_head([])),
            },
        };

        debug!("WeightedAtomSweep initialization complete");
        result
    }

    // TODO: map can be limited to a subset of the map
    #[instrument(skip_all, name = "sweep.spawn")]
    pub fn spawn(self) -> SweepController<H> {
        debug!("spawning WeightedAtomSweep threads");

        let (atom_sender, atom_reciever) = mpsc::channel::<AtomPosition>();
        let engine = self.traversal.clone();
        let sender = atom_sender.clone();
        let map = self.map.inner.clone();
        let operation_count = self.operations.len();

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_traversal = shutdown.clone();
        let shutdown_operations = shutdown.clone();

        let mut handles = Vec::new();

        // spawn traversal thread
        let traversal_handle = std::thread::spawn(move || {
            let traversal_span = span!(Level::DEBUG, "traversal_thread");
            let _enter = traversal_span.enter();

            debug!("traversal thread started - entering sampling loop");

            loop {
                // Check for shutdown signal
                if shutdown_traversal.load(Ordering::Relaxed) {
                    debug!("shutdown signal received, exiting traversal loop");
                    break;
                }

                // get access to a read zipper at root for sampling
                match self.map.read_zipper_at_borrowed_path(&[]) {
                    Ok(traverse_zp) => {
                        trace!("acquired read zipper for sampling");
                        match engine.next_atom(traverse_zp) {
                            Ok(atom_path) => {
                                debug!(atom_path_len = atom_path.len(), "atom sampled via traversal");
                                if sender.send(atom_path).is_err() {
                                    debug!("operations thread terminated - stopping traversal");
                                    break;
                                }
                            }
                            Err(e) => {
                                // Log and continue (resilient mode as per user requirement)
                                trace!("error during atom traversal: {:?}", e);
                                // Don't break - continue sampling
                            }
                        }
                    }
                    Err(e) => {
                        trace!("failed to acquire read zipper: {:?}", e);
                        // Continue trying in case of transient errors
                    }
                }
            }

            debug!("traversal thread completed");
            drop(sender); // Signal operations thread that no more atoms will be sent
        });

        handles.push(traversal_handle);

        // handle traversed atoms
        let operations_handle = std::thread::spawn(move || {
            let operations_span = span!(Level::DEBUG, "operations_thread", operation_count);
            let _enter = operations_span.enter();

            debug!("operations thread started - entering processing loop");

            loop {
                match atom_reciever.recv() {
                    Ok(atom_path) => {
                        let atom = Arc::new(atom_path);
                        debug!(atom_len = atom.len(), operation_count, "processing atom with operations");

                        for (idx, op) in self.operations.iter().enumerate() {
                            let op_span = span!(Level::TRACE, "operation", index = idx, name = op.name());
                            let _op_enter = op_span.enter();

                            trace!("executing operation");

                            // Catch panics to prevent thread death
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                op.transform(atom.clone().into());
                            }));

                            if let Err(e) = result {
                                // Log panic but continue with next operation (resilient mode)
                                trace!("operation panicked: {:?}", e);
                            } else {
                                trace!("operation completed");
                            }
                        }

                        debug!("all operations completed for atom");
                    }
                    Err(_) => {
                        debug!("traversal complete - channel closed, no more atoms");
                        break;
                    }
                }
            }

            debug!("operations thread completed");
            shutdown_operations.store(true, Ordering::Relaxed); // Signal completion
        });

        handles.push(operations_handle);

        debug!("spawn operation complete, returning controller");

        SweepController {
            map,
            handles,
            shutdown_signal: shutdown,
        }
    }
}

impl<T, O, H> OperationObserver<H, O> for WeightedAtomSweep<T, O, H>
where
    T: SweepTransversalEngine<H>,
    O: KernelOperation<H>,
    H: AtomHeader,
{
    #[instrument(skip_all, name = "sweep.subscribe", fields(op_name = operation.name()))]
    fn subscribe(&mut self, operation: O) {
        let total_operations = self.operations.len() + 1;
        debug!(operation_name = operation.name(), total_operations, "subscribing operation");
        self.operations.push(operation);
        trace!("operation subscribed successfully");
    }

    #[instrument(skip_all, name = "sweep.unsubscribe", fields(op_name = operation.name()))]
    fn unsubscribe(&mut self, operation: O) {
        let initial_count = self.operations.len();
        debug!(operation_name = operation.name(), initial_count, "unsubscribing operation");
        self.operations.retain(|op| op != &operation);
        let final_count = self.operations.len();
        let removed = initial_count - final_count;
        debug!(removed, final_count, "operation unsubscribe complete");
    }
}
