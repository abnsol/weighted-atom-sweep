//! Tests of the loop, threads and operations are working

use weighted_atom_sweep::{
    AtomHeader, Operation, OperationObserver, TraversalEngine, WeightedAtomSweep,
    WeightedAtomSweepSettings,
};

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct Header {
    value: u64,
    agg: u64,
}

impl AtomHeader for Header {}

mod engines {
    use pathmap::zipper::ReadZipperTracked;
    use tracing::debug;
    use weighted_atom_sweep::{AtomPosition, TraversalError};

    use super::Header;

    pub fn engine1(_z: ReadZipperTracked<Header>) -> Result<AtomPosition, TraversalError> {
        // debug!("engine1 traversal running");
        std::thread::sleep(std::time::Duration::from_millis(3000));
        // debug!("engine1 sampled atom");
        Ok(vec![0])
    }

    pub fn engine2(_z: ReadZipperTracked<Header>) -> Result<AtomPosition, TraversalError> {
        // debug!("engine2 traversal running");
        std::thread::sleep(std::time::Duration::from_millis(2500));
        // debug!("engine2 sampled atom");
        Ok(vec![1])
    }
}

mod operations {
    use std::sync::Arc;
    use weighted_atom_sweep::AtomPosition;

    pub fn log_atom(_atom: Arc<AtomPosition>) {
        // debug!(_atom_len = _atom.len(), "operation: received atom");
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }

    pub fn process_atom(_atom: Arc<AtomPosition>) {
        // debug!(_atom_len = _atom.len(), "operation: processing atom");
        std::thread::sleep(std::time::Duration::from_millis(5000));
    }

    pub fn validate_atom(_atom: Arc<AtomPosition>) {
        // debug!(_atom_len = _atom.len(), "operation: validating atom");
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    pub fn transform_atom(_atom: Arc<AtomPosition>) {
        // debug!(_atom_len = _atom.len(), "operation: transforming atom");
        std::thread::sleep(std::time::Duration::from_millis(800));
    }

    pub fn persist_atom(_atom: Arc<AtomPosition>) {
        // debug!(_atom_len = _atom.len(), "operation: persisting atom");
        std::thread::sleep(std::time::Duration::from_millis(600));
    }

    pub const LOG_ATOM_FN: fn(Arc<AtomPosition>) = log_atom;
    pub const PROCESS_ATOM_FN: fn(Arc<AtomPosition>) = process_atom;
    pub const VALIDATE_ATOM_FN: fn(Arc<AtomPosition>) = validate_atom;
    pub const TRANSFORM_ATOM_FN: fn(Arc<AtomPosition>) = transform_atom;
    pub const PERSIST_ATOM_FN: fn(Arc<AtomPosition>) = persist_atom;
}

#[test]
fn smoke_test() {
    // Initialize tracing for this test (logs disabled - uncomment to enable)
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let mut sweep = WeightedAtomSweep::<Header>::new(WeightedAtomSweepSettings::default());

    // Create and add first engine with operations
    let engine1 = TraversalEngine::new("engine1", engines::engine1);
    let process1 = sweep.add_engine(engine1);

    let log_op = Operation::new("log_atom", &operations::LOG_ATOM_FN);
    let process_op = Operation::new("process_atom", &operations::PROCESS_ATOM_FN);
    let validate_op = Operation::new("validate_atom", &operations::VALIDATE_ATOM_FN);
    let transform_op = Operation::new("transform_atom", &operations::TRANSFORM_ATOM_FN);
    let persist_op = Operation::new("persist_atom", &operations::PERSIST_ATOM_FN);

    process1.subscribe(log_op);
    process1.subscribe(process_op);
    process1.subscribe(validate_op);
    process1.subscribe(transform_op);
    process1.subscribe(persist_op);

    // Create and add second engine with operations
    let engine2 = TraversalEngine::new("engine2", engines::engine2);
    let process2 = sweep.add_engine(engine2);

    let log_op2 = Operation::new("log_atom", &operations::LOG_ATOM_FN);
    let validate_op2 = Operation::new("validate_atom", &operations::VALIDATE_ATOM_FN);
    let persist_op2 = Operation::new("persist_atom", &operations::PERSIST_ATOM_FN);

    process2.subscribe(log_op2);
    process2.subscribe(validate_op2);
    process2.subscribe(persist_op2);

    // tracing::info!(
    //     "[SMOKE_TEST] sweep configured with {} process(es)",
    //     sweep.process_count()
    // );

    // Spawn the sweep threads
    let controller = sweep.spawn();
    // tracing::info!(
    //     "[SMOKE_TEST] sweep spawned with {} thread(s)",
    //     controller.thread_count()
    // );

    // Let it run briefly then shutdown
    std::thread::sleep(std::time::Duration::from_millis(10_000));
    let result = controller.shutdown();

    assert!(result.is_ok(), "sweep shutdown should succeed");
    // tracing::info!("[SMOKE_TEST] smoke test completed successfully");
}
