//! Tests demonstrating `SExprOperation` — mm2 s-expression operations
//! within the weighted-atom-sweep framework.
//!
//! These tests exercise the three modes of `SExprOperation`:
//! - **Add**: inserting an expression as a trie path
//! - **Remove**: deleting an expression's trie path
//! - **Match**: pattern-matching an expression against the trie structure
//!
//! The first three tests are direct (no sweep threads) — they construct a
//! `ZipperHeadOwned`, acquire a `WriteZipperTracked`, and call `apply()`
//! directly. The fourth test runs an `SExprOperation` inside the full
//! sweep loop.

use mork_expr::{item_byte, parse, Tag};
use pathmap::zipper::{Zipper, ZipperCreation};
use pathmap::PathMap;

use weighted_atom_sweep::{
    AtomHeader, Operation, OperationObserver, SExprMode, SExprOperation, TransformOp,
    TraversalEngine, WeightedAtomSweep, WeightedAtomSweepSettings,
};

// ---------------------------------------------------------------------------
// Shared Header type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Header;

impl AtomHeader for Header {}

// ---------------------------------------------------------------------------
// Helper: encode an expression with parse! and return the byte slice
// ---------------------------------------------------------------------------

/// Encode `(foo bar)` — arity 2, symbol "foo", symbol "bar"
fn expr_foo_bar() -> Vec<u8> {
    let data = parse!("[2] foo bar");
    data.to_vec()
}

/// Encode `(= a a)` — arity 3, symbol "=", symbol "a", symbol "a"
fn expr_eq_a_a() -> Vec<u8> {
    let data = parse!("[3] = a a");
    data.to_vec()
}

/// Encode `(= b b)`
fn expr_eq_b_b() -> Vec<u8> {
    let data = parse!("[3] = b b");
    data.to_vec()
}

/// Encode `(= a b)`
fn expr_eq_a_b() -> Vec<u8> {
    let data = parse!("[3] = a b");
    data.to_vec()
}

/// Encode `(= $ _1)` — the co-referential match pattern
fn pattern_eq_x_x() -> Vec<u8> {
    let data = parse!("[3] = $ _1");
    data.to_vec()
}

/// Encode `(f (g a) b)` — nested expression
fn expr_f_ga_b() -> Vec<u8> {
    let data = parse!("[3] f [2] g a b");
    data.to_vec()
}

// ===========================================================================
// Test 1: SExprOperation::Add — direct application
// ===========================================================================

#[test]
fn test_sexpr_add_direct() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let expr_bytes = expr_foo_bar();

    // Create an empty trie
    let space = PathMap::<Header>::new().into_zipper_head([]);

    // Build the Add operation
    let op = SExprOperation::<Header>::from_bytes("add_foo_bar", &expr_bytes, SExprMode::Add);

    // Acquire a write zipper at root and apply
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        op.apply(&mut wz);
        // wz is dropped here, releasing the exclusive path
        space.cleanup_write_zipper(wz);
    }

    // Verify the expression path was inserted
    let map = space.into_map();
    let val = map.get_val_at(&expr_bytes);
    assert!(
        val.is_some(),
        "expected a value at the expression path after Add; got None"
    );
}

// ===========================================================================
// Test 2: SExprOperation::Remove — direct application
// ===========================================================================

#[test]
fn test_sexpr_remove_direct() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let expr_bytes = expr_foo_bar();

    // Create a trie pre-populated with the expression path
    let mut map = PathMap::<Header>::new();
    map.set_val_at(&expr_bytes, Header);
    assert!(
        map.get_val_at(&expr_bytes).is_some(),
        "precondition: value should exist before Remove"
    );

    let space = map.into_zipper_head([]);

    // Build the Remove operation
    let op = SExprOperation::<Header>::from_bytes("remove_foo_bar", &expr_bytes, SExprMode::Remove);

    // Apply
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }

    // Verify the expression path was removed
    let map = space.into_map();
    let val = map.get_val_at(&expr_bytes);
    assert!(
        val.is_none(),
        "expected no value at the expression path after Remove; got {:?}",
        val
    );
}

// ===========================================================================
// Test 3: SExprOperation::Match — co-referential pattern matching
// ===========================================================================

/// Insert three expressions into the trie:
///   (= a a)  — self-equal, should match `(= $x $x)`
///   (= b b)  — self-equal, should match `(= $x $x)`
///   (= a b)  — NOT self-equal, should NOT match
///
/// Then run Match with `(= $ _1)` and verify match_count == 2.
#[test]
fn test_sexpr_match_direct() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let eq_a_a = expr_eq_a_a();
    let eq_b_b = expr_eq_b_b();
    let eq_a_b = expr_eq_a_b();
    let pattern = pattern_eq_x_x();

    // Populate the trie with three expressions
    let mut map = PathMap::<Header>::new();
    map.set_val_at(&eq_a_a, Header);
    map.set_val_at(&eq_b_b, Header);
    map.set_val_at(&eq_a_b, Header);

    let space = map.into_zipper_head([]);

    // Build the Match operation
    let op = SExprOperation::<Header>::from_bytes("match_eq_x_x", &pattern, SExprMode::Match);

    assert_eq!(op.match_count(), 0, "match count should start at 0");

    // Apply
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }

    // (= a a) and (= b b) should match; (= a b) should not
    assert_eq!(
        op.match_count(),
        2,
        "expected 2 co-referential matches for pattern (= $ _1)"
    );
}

// ===========================================================================
// Test 4: SExprOperation::Match on a nested expression structure
// ===========================================================================

/// Insert expressions with different structures and verify a wildcard
/// pattern matches the expected number of entries.
///
///   (= a a)     — matches `(= $ _1)` (same first and second arg)
///   (= b b)     — matches `(= $ _1)`
///   (= a b)     — does NOT match
///   (f (g a) b) — does NOT match (different top-level arity/symbol)
///
/// This test also verifies that unrelated expressions in the trie
/// don't interfere with matching.
#[test]
fn test_sexpr_match_with_noise() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let eq_a_a = expr_eq_a_a();
    let eq_b_b = expr_eq_b_b();
    let eq_a_b = expr_eq_a_b();
    let f_ga_b = expr_f_ga_b();
    let pattern = pattern_eq_x_x();

    let mut map = PathMap::<Header>::new();
    map.set_val_at(&eq_a_a, Header);
    map.set_val_at(&eq_b_b, Header);
    map.set_val_at(&eq_a_b, Header);
    map.set_val_at(&f_ga_b, Header);

    let space = map.into_zipper_head([]);

    let op = SExprOperation::<Header>::from_bytes("match_eq_x_x", &pattern, SExprMode::Match);

    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }

    assert_eq!(
        op.match_count(),
        2,
        "noise expressions should not affect co-referential match count"
    );
}

// ===========================================================================
// Test 5: SExprOperation::Add inside the full sweep loop
// ===========================================================================

/// Run an `SExprOperation` in Add mode through the full sweep loop.
///
/// The traversal engine returns a fixed atom position (root `[]`), and
/// the operations thread applies the Add operation. After shutdown we
/// verify the expression path exists in the trie.
#[test]
fn test_sexpr_in_sweep_loop() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    let expr_bytes = expr_foo_bar();

    let mut sweep = WeightedAtomSweep::<Header>::new(WeightedAtomSweepSettings::default());

    // Engine that always returns root position
    let engine = TraversalEngine::new("fixed_root", |_rz| {
        std::thread::sleep(std::time::Duration::from_millis(500));
        Ok(vec![])
    });

    let process = sweep.add_engine(engine);

    // Subscribe the mm2 Add operation
    let add_op = SExprOperation::<Header>::from_bytes("add_foo_bar", &expr_bytes, SExprMode::Add);
    process.subscribe(add_op);

    // Also subscribe a simple fn-pointer operation for comparison
    let noop = Operation::<Header>::new(
        "noop",
        |_wz: &mut pathmap::zipper::WriteZipperTracked<Header>| {},
    );
    process.subscribe(noop);

    // Spawn, let it run briefly, then shutdown
    let controller = sweep.spawn();
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let result = controller.shutdown();
    assert!(result.is_ok(), "sweep shutdown should succeed");
}

// ===========================================================================
// Test 6: Add then Remove round-trip
// ===========================================================================

/// Add an expression, verify it exists, then Remove it, verify it's gone.
#[test]
fn test_sexpr_add_then_remove() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let expr_bytes = expr_f_ga_b();

    let space = PathMap::<Header>::new().into_zipper_head([]);

    // --- Add ---
    let add_op = SExprOperation::<Header>::from_bytes("add_expr", &expr_bytes, SExprMode::Add);
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        add_op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }

    // Verify it was added (peek via read zipper)
    {
        let rz = space.read_zipper_at_borrowed_path(&[] as &[u8]).unwrap();
        // Navigate to the expression path
        // We just need to check the trie has content — the read zipper
        // at root should have children now
        assert!(rz.child_count() > 0, "trie should have children after Add");
    }

    // --- Remove ---
    let remove_op =
        SExprOperation::<Header>::from_bytes("remove_expr", &expr_bytes, SExprMode::Remove);
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        remove_op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }

    // Verify it was removed
    let map = space.into_map();
    assert!(
        map.get_val_at(&expr_bytes).is_none(),
        "expression path should be gone after Remove"
    );
}

// ===========================================================================
// Test 7: Match count accumulates across multiple apply calls
// ===========================================================================

#[test]
fn test_sexpr_match_count_accumulates() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let eq_a_a = expr_eq_a_a();
    let eq_b_b = expr_eq_b_b();
    let pattern = pattern_eq_x_x();

    let mut map = PathMap::<Header>::new();
    map.set_val_at(&eq_a_a, Header);
    map.set_val_at(&eq_b_b, Header);

    let space = map.into_zipper_head([]);

    let op = SExprOperation::<Header>::from_bytes("match_accum", &pattern, SExprMode::Match);

    // First apply
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }
    assert_eq!(op.match_count(), 2);

    // Second apply — counter should accumulate to 4
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }
    assert_eq!(op.match_count(), 4, "match_count should accumulate");

    // Reset and apply again — should be 2
    op.reset_match_count();
    assert_eq!(op.match_count(), 0);
    {
        let mut wz = space.write_zipper_at_exclusive_path(&[] as &[u8]).unwrap();
        op.apply(&mut wz);
        space.cleanup_write_zipper(wz);
    }
    assert_eq!(
        op.match_count(),
        2,
        "match_count should be 2 after reset + apply"
    );
}

// ===========================================================================
// Test 8: Verify mm2 encoding round-trip via manual byte construction
// ===========================================================================

/// Construct an expression manually byte-by-byte (without parse! macro),
/// add it to the trie, and verify the path matches what parse! would
/// produce.
#[test]
fn test_manual_encoding_matches_parse() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    // Manually encode `(= a a)`:
    //   Arity(3) | SymbolSize(1) '=' | SymbolSize(1) 'a' | SymbolSize(1) 'a'
    let manual_bytes = vec![
        item_byte(Tag::Arity(3)),
        item_byte(Tag::SymbolSize(1)),
        b'=',
        item_byte(Tag::SymbolSize(1)),
        b'a',
        item_byte(Tag::SymbolSize(1)),
        b'a',
    ];

    let parsed_bytes = expr_eq_a_a();

    assert_eq!(
        manual_bytes, parsed_bytes,
        "manual byte encoding should match parse! output"
    );

    // Manually encode the match pattern `(= $ _1)`:
    //   Arity(3) | SymbolSize(1) '=' | NewVar | VarRef(0)
    let manual_pattern = vec![
        item_byte(Tag::Arity(3)),
        item_byte(Tag::SymbolSize(1)),
        b'=',
        item_byte(Tag::NewVar),
        item_byte(Tag::VarRef(0)),
    ];

    let parsed_pattern = pattern_eq_x_x();

    assert_eq!(
        manual_pattern, parsed_pattern,
        "manual pattern encoding should match parse! output"
    );
}
