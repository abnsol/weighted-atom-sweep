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
    use weighted_atom_sweep::{AtomPosition, TraversalError};

    use super::Header;

    pub fn engine1(_z: ReadZipperTracked<Header>) -> Result<AtomPosition, TraversalError> {
        std::thread::sleep(std::time::Duration::from_millis(3000));
        Ok(vec![0])
    }

    pub fn engine2(_z: ReadZipperTracked<Header>) -> Result<AtomPosition, TraversalError> {
        std::thread::sleep(std::time::Duration::from_millis(2500));
        Ok(vec![1])
    }
}

mod operations {
    use super::Header;
    use pathmap::zipper::WriteZipperTracked;

    pub fn log_atom(_wz: &mut WriteZipperTracked<Header>, _atom_path: &[u8]) {
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }

    pub fn process_atom(_wz: &mut WriteZipperTracked<Header>, _atom_path: &[u8]) {
        std::thread::sleep(std::time::Duration::from_millis(5000));
    }

    pub fn validate_atom(_wz: &mut WriteZipperTracked<Header>, _atom_path: &[u8]) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    pub fn transform_atom(_wz: &mut WriteZipperTracked<Header>, _atom_path: &[u8]) {
        std::thread::sleep(std::time::Duration::from_millis(800));
    }

    pub fn persist_atom(_wz: &mut WriteZipperTracked<Header>, _atom_path: &[u8]) {
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
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

    let log_op = Operation::<Header>::new("log_atom", operations::log_atom);
    let process_op = Operation::<Header>::new("process_atom", operations::process_atom);
    let validate_op = Operation::<Header>::new("validate_atom", operations::validate_atom);
    let transform_op = Operation::<Header>::new("transform_atom", operations::transform_atom);
    let persist_op = Operation::<Header>::new("persist_atom", operations::persist_atom);

    process1.subscribe(log_op);
    process1.subscribe(process_op);
    process1.subscribe(validate_op);
    process1.subscribe(transform_op);
    process1.subscribe(persist_op);

    // Create and add second engine with operations
    let engine2 = TraversalEngine::new("engine2", engines::engine2);
    let process2 = sweep.add_engine(engine2);

    let log_op2 = Operation::<Header>::new("log_atom", operations::log_atom);
    let validate_op2 = Operation::<Header>::new("validate_atom", operations::validate_atom);
    let persist_op2 = Operation::<Header>::new("persist_atom", operations::persist_atom);

    process2.subscribe(log_op2);
    process2.subscribe(validate_op2);
    process2.subscribe(persist_op2);

    // Spawn the sweep threads
    let controller = sweep.spawn();

    // Let it run briefly then shutdown
    std::thread::sleep(std::time::Duration::from_millis(10_000));
    let result = controller.shutdown();

    assert!(result.is_ok(), "sweep shutdown should succeed");
}
