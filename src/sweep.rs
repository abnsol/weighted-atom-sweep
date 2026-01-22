use crate::map::WeightedMap;
use crate::operation::{Operation, OperationObserver};
use crate::traversal::TransversalEngine;
use pathmap::PathMap;
use pathmap::zipper::{ZipperCreation, ZipperHeadOwned};
use std::sync::{Arc, mpsc};
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
    pub fn spawn(self) -> Arc<ZipperHeadOwned<H>> {
        debug!("spawning WeightedAtomSweep threads");

        let (atom_sender, atom_reciever) = mpsc::channel::<AtomPosition>();
        let engine = self.traversal.clone();
        let sender = atom_sender.clone();
        let map = self.map.inner.clone();
        let operation_count = self.operations.len();

        // spawn traversal thread
        std::thread::spawn(move || {
            let traversal_span = span!(Level::DEBUG, "traversal_thread");
            let _enter = traversal_span.enter();

            debug!("traversal thread started");

            // get access to a read zipper
            match self.map.read_zipper_at_borrowed_path(&[]) {
                Ok(traverse_zp) => {
                    trace!("acquired read zipper at root");
                    match engine.next_atom(traverse_zp) {
                        Ok(atom_path) => {
                            debug!(atom_path_len = atom_path.len(), "atom found via traversal");
                            if sender.send(atom_path).is_err() {
                                trace!("failed to send atom - receiver dropped");
                            }
                        }
                        Err(_) => {
                            trace!("error during atom traversal");
                        }
                    }
                }
                Err(_) => {
                    trace!("failed to acquire read zipper at root");
                }
            }

            debug!("traversal thread completed");
        });

        // handle traversed atom
        std::thread::spawn(move || {
            let operations_span = span!(Level::DEBUG, "operations_thread", operation_count);
            let _enter = operations_span.enter();

            debug!("operations thread started");

            match atom_reciever.recv() {
                Ok(atom_path) => {
                    let atom = Arc::new(atom_path);
                    debug!(atom_len = atom.len(), operation_count, "processing atom with operations");

                    for (idx, op) in self.operations.iter().enumerate() {
                        let op_span = span!(Level::TRACE, "operation", index = idx, name = op.name());
                        let _op_enter = op_span.enter();

                        trace!("executing operation");
                        op.transform(atom.clone().into());
                        trace!("operation completed");
                    }

                    debug!("all operations completed");
                }
                Err(_) => {
                    trace!("failed to receive atom from traversal thread");
                }
            }

            debug!("operations thread completed");
        });

        debug!("spawn operation complete, returning map");
        map
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
