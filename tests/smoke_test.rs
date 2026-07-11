//! Tests that the sweep loop, threads and operations work end-to-end.

use std::time::Duration;

use pathmap::zipper::ReadZipperTracked;
use weighted_atom_sweep::{
    AtomPosition, Operation, OperationObserver, TraversalEngine, TraversalError,
    WeightedAtomSweep, WeightedAtomSweepSettings,
};

// --- Custom traversal engines (sleep, then return a fixed path) ---

struct Engine1;
impl TraversalEngine for Engine1 {
    fn name(&self) -> &str { "engine1" }
    fn next_atom(&self, _z: ReadZipperTracked<u64>) -> Result<AtomPosition, TraversalError> {
        std::thread::sleep(Duration::from_millis(3000));
        Ok(vec![0])
    }
}

struct Engine2;
impl TraversalEngine for Engine2 {
    fn name(&self) -> &str { "engine2" }
    fn next_atom(&self, _z: ReadZipperTracked<u64>) -> Result<AtomPosition, TraversalError> {
        std::thread::sleep(Duration::from_millis(2500));
        Ok(vec![1])
    }
}

// --- Operations (each just sleeps to simulate work) ---

mod operations {
    use pathmap::zipper::WriteZipperTracked;
    use std::time::Duration;

    pub fn log_atom(_wz: &mut WriteZipperTracked<u64>, _atom_path: &[u8]) {
        std::thread::sleep(Duration::from_millis(1000));
    }

    pub fn process_atom(_wz: &mut WriteZipperTracked<u64>, _atom_path: &[u8]) {
        std::thread::sleep(Duration::from_millis(5000));
    }

    pub fn validate_atom(_wz: &mut WriteZipperTracked<u64>, _atom_path: &[u8]) {
        std::thread::sleep(Duration::from_millis(500));
    }

    pub fn transform_atom(_wz: &mut WriteZipperTracked<u64>, _atom_path: &[u8]) {
        std::thread::sleep(Duration::from_millis(800));
    }

    pub fn persist_atom(_wz: &mut WriteZipperTracked<u64>, _atom_path: &[u8]) {
        std::thread::sleep(Duration::from_millis(600));
    }
}

fn op(name: &'static str, f: fn(&mut WriteZipperTracked<u64>, &[u8])) -> Box<Operation> {
    Box::new(Operation::new(name, f))
}

#[test]
fn smoke_test() {
    // Initialize tracing for this test (logs disabled - uncomment to enable)
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let mut sweep = WeightedAtomSweep::new(WeightedAtomSweepSettings::default());

    // First engine with five operations (custom engine handed in directly).
    {
        let process1 = sweep.add_engine("engine1", "cpq");
        process1.subscribe(op("log_atom", operations::log_atom));
        process1.subscribe(op("process_atom", operations::process_atom));
        process1.subscribe(op("validate_atom", operations::validate_atom));
        process1.subscribe(op("transform_atom", operations::transform_atom));
        process1.subscribe(op("persist_atom", operations::persist_atom));
    }

    // Second engine with three operations.
    {
        let process2 = sweep.add_engine("engine2", "random_walk");
        process2.subscribe(op("log_atom", operations::log_atom));
        process2.subscribe(op("validate_atom", operations::validate_atom));
        process2.subscribe(op("persist_atom", operations::persist_atom));
    }

    // Spawn the sweep threads.
    let _name = sweep.spawn();

    // Let it run briefly then shutdown.
    std::thread::sleep(Duration::from_millis(10_000));
    let result = sweep.shutdown_all();

    assert!(result.is_some(), "sweep shutdown should succeed");
}

#[test]
fn test_pause_resume() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let mut sweep = WeightedAtomSweep::new(WeightedAtomSweepSettings::default());

    // Slow engine to make the pause test reliable
    struct SlowEngine;
    impl TraversalEngine for SlowEngine {
        fn name(&self) -> &str { "slow_engine" }
        fn next_atom(&self, _z: ReadZipperTracked<u64>) -> Result<AtomPosition, TraversalError> {
            std::thread::sleep(Duration::from_millis(100));
            Ok(vec![0])
        }
    }

    let process = sweep.add_engine("engine_slow", "cpq");
    process.subscribe(op("log_atom", operations::log_atom));

    let _name = sweep.spawn();
    std::thread::sleep(Duration::from_millis(200));

    // Pause and verify quiescence
    let path_map = sweep.pause_all();
    // The controller is now paused — verify all threads parked
    for ctrl in sweep.controllers.values() {
        assert!(
            ctrl.parked_count() >= ctrl.thread_count(),
            "all threads should be parked after pause"
        );
    }

    // Resume and shutdown cleanly
    sweep.resume_all(path_map);
    std::thread::sleep(Duration::from_millis(100));
    let result = sweep.shutdown_all();
    assert!(result.is_some(), "sweep shutdown should succeed after pause/resume");
}
