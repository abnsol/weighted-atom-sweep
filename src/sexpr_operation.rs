use std::marker::PhantomData;

use mork_expr::{Expr, Tag, byte_item};
use pathmap::zipper::{WriteZipperTracked, Zipper, ZipperMoving, ZipperWriting};
use tracing::{debug, trace};

use crate::operation::TransformOp;
use crate::sweep::AtomHeader;

/// The mode of operation for an [`SExprOperation`].
///
/// Determines how the mm2 s-expression is applied to the subtrie
/// accessible via the write zipper.
#[derive(Clone, Debug)]
pub enum SExprMode {
    /// Add the expression as a path in the subtrie, setting a value at
    /// the leaf. The expression bytes become the trie path, descending
    /// from the write zipper's focus.
    Add,

    /// Remove the expression's path from the subtrie. Navigates to the
    /// path defined by the expression bytes and removes the value (and
    /// prunes empty branches).
    Remove,

    /// Match the expression as a pattern against the subtrie.
    /// Walks the existing trie structure using the expression's tag
    /// encoding. Concrete elements (symbols, arities) must match
    /// exactly. Variables (`NewVar`, `VarRef`) act as wildcards that
    /// match any subtrie.
    ///
    /// This is a simplified version of MORK's `coreferential_transition`
    /// that handles single-pattern matching without the full product
    /// zipper machinery.
    Match,
}

/// An operation that uses an mm2 s-expression to define its transformation.
///
/// The s-expression is stored as an owned byte vector (copied from the
/// original `Expr`). The [`SExprMode`] determines how the expression is
/// applied to the subtrie.
///
/// # Thread Safety
///
/// `SExprOperation` is `Send + Sync` because the expression data is owned
/// (`Vec<u8>`) and the mode is an enum with no interior mutability.
///
/// # Example
/// ```ignore
/// use mork_expr::{Expr, parse};
/// use weighted_atom_sweep::{SExprOperation, SExprMode, AtomHeader};
///
/// #[derive(Debug, Clone, Default)]
/// struct Header;
/// impl AtomHeader for Header {}
///
/// // Parse an mm2 expression
/// let expr_bytes = parse!("(foo bar)");
/// let expr = Expr { ptr: expr_bytes.as_ptr() as *mut u8 };
///
/// // Create an add operation
/// let op = SExprOperation::<Header>::new("add_foo_bar", expr, SExprMode::Add);
/// ```
pub struct SExprOperation<H: AtomHeader> {
    name: String,
    /// Owned copy of the mm2 expression bytes.
    expr_data: Vec<u8>,
    /// The operation mode.
    mode: SExprMode,
    _phantom: PhantomData<H>,
}

impl<H: AtomHeader> SExprOperation<H> {
    /// Create a new s-expression operation.
    ///
    /// # Arguments
    /// * `name` - A descriptive name for tracing and identification.
    /// * `expr` - The mm2 s-expression. Its byte span is copied into owned storage.
    /// * `mode` - How the expression should be applied ([`SExprMode`]).
    ///
    /// # Safety
    /// The `expr.ptr` must point to a valid mm2-encoded expression.
    pub fn new(name: impl Into<String>, expr: Expr, mode: SExprMode) -> Self {
        let name = name.into();
        let span = unsafe { expr.span().as_ref().unwrap() };
        let expr_data = span.to_vec();
        debug!(
            name = %name,
            expr_len = expr_data.len(),
            ?mode,
            "creating SExprOperation"
        );
        Self {
            name,
            expr_data,
            mode,
            _phantom: PhantomData,
        }
    }

    /// Create a new s-expression operation directly from raw bytes.
    ///
    /// This avoids the need to construct an `Expr` pointer -- the bytes
    /// are taken as-is.
    ///
    /// # Arguments
    /// * `name` - A descriptive name.
    /// * `expr_bytes` - The raw mm2-encoded expression bytes.
    /// * `mode` - How the expression should be applied.
    pub fn from_bytes(name: impl Into<String>, expr_bytes: &[u8], mode: SExprMode) -> Self {
        Self {
            name: name.into(),
            expr_data: expr_bytes.to_vec(),
            mode,
            _phantom: PhantomData,
        }
    }

    /// Returns a reference to the stored expression bytes.
    pub fn expr_bytes(&self) -> &[u8] {
        &self.expr_data
    }

    /// Returns the operation mode.
    pub fn mode(&self) -> &SExprMode {
        &self.mode
    }

    /// Apply in Add mode: descend along the expression's byte path and
    /// set a value at the leaf.
    fn apply_add(&self, wz: &mut WriteZipperTracked<H>)
    where
        H: Default,
    {
        let data = &self.expr_data[..];
        if data.is_empty() {
            return;
        }
        trace!(
            name = %self.name,
            path_len = data.len(),
            "SExprOperation::apply_add - descending to expression path"
        );
        wz.descend_to(data);
        wz.set_val(H::default());
        wz.reset();
    }

    /// Apply in Remove mode: navigate to the expression's byte path
    /// and remove the value (with pruning).
    fn apply_remove(&self, wz: &mut WriteZipperTracked<H>) {
        let data = &self.expr_data[..];
        if data.is_empty() {
            return;
        }
        trace!(
            name = %self.name,
            path_len = data.len(),
            "SExprOperation::apply_remove - descending to expression path"
        );
        wz.descend_to(data);
        if wz.is_val() {
            wz.remove_val(true);
        }
        wz.reset();
    }

    /// Apply in Match mode: walk the subtrie matching the expression pattern.
    ///
    /// This implements a simplified version of coreferential transition:
    /// - `SymbolSize(n)` followed by n bytes: must match exactly in the trie
    /// - `Arity(a)`: must match exactly, then recursively match a children
    /// - `NewVar`: matches any single item (symbol, arity subtree, or variable)
    ///   in the trie, enumerating all branches
    /// - `VarRef(k)`: re-matches the bytes captured at reference k
    ///
    /// For each complete match, the write zipper is positioned at the matched
    /// path.
    fn apply_match(&self, wz: &mut WriteZipperTracked<H>) {
        let data = &self.expr_data[..];
        if data.is_empty() {
            return;
        }

        trace!(
            name = %self.name,
            expr_len = data.len(),
            "SExprOperation::apply_match - starting pattern match"
        );

        let mut references: Vec<usize> = Vec::new();
        let mut match_count = 0usize;

        self.match_item(wz, 0, &mut references, &mut match_count);

        debug!(
            name = %self.name,
            match_count,
            "SExprOperation::apply_match complete"
        );
    }

    /// Match a single expression item starting at `expr_offset` against
    /// the current trie position. After matching, continues with whatever
    /// is at `expr_offset + item_size`.
    ///
    /// When the entire expression is consumed (expr_offset >= data.len()),
    /// a match is recorded.
    fn match_item(
        &self,
        wz: &mut WriteZipperTracked<H>,
        expr_offset: usize,
        references: &mut Vec<usize>,
        match_count: &mut usize,
    ) {
        let data = &self.expr_data;
        if expr_offset >= data.len() {
            // Full expression matched
            *match_count += 1;
            trace!(
                name = %self.name,
                match_count = *match_count,
                "pattern matched"
            );
            return;
        }

        let tag = byte_item(data[expr_offset]);
        match tag {
            Tag::NewVar => {
                // Variable matches any item in the trie at this position.
                // Record the current path length for co-referential binding.
                let ref_idx = references.len();
                references.push(wz.path().len());

                // Enumerate all children at this trie position
                let cm = wz.child_mask();
                let mut it = cm.iter();
                while let Some(b) = it.next() {
                    let child_tag = byte_item(b);
                    match child_tag {
                        Tag::SymbolSize(size) => {
                            // Symbol: descend to tag byte, then enumerate
                            // all size-byte paths manually
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                self.enumerate_k_paths(
                                    wz,
                                    size as usize,
                                    expr_offset + 1,
                                    references,
                                    match_count,
                                );
                            }
                            wz.ascend_byte();
                        }
                        Tag::Arity(_) => {
                            // Arity node: descend to tag byte, then match
                            // `a` children (consuming `a` items from the
                            // wildcard's perspective — but since NewVar
                            // matches a SINGLE complete item, we need to
                            // skip over the entire arity subtree.
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                // For a wildcard matching an arity node,
                                // we need to traverse past the entire
                                // sub-expression. We do this by continuing
                                // the match from the next expression position
                                // (expr_offset + 1) which is what follows
                                // the NewVar.
                                self.match_item(wz, expr_offset + 1, references, match_count);
                            }
                            wz.ascend_byte();
                        }
                        _ => {
                            // NewVar or VarRef bytes in the trie data
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                self.match_item(wz, expr_offset + 1, references, match_count);
                            }
                            wz.ascend_byte();
                        }
                    }
                }

                references.truncate(ref_idx);
            }

            Tag::VarRef(k) => {
                // Re-match the bytes captured at reference k.
                if (k as usize) < references.len() {
                    let ref_path_start = references[k as usize];
                    let current_path = wz.path().to_vec();

                    if ref_path_start < current_path.len() {
                        let bound_copy = current_path[ref_path_start..].to_vec();
                        wz.descend_to(&bound_copy);
                        if wz.path_exists() {
                            self.match_item(wz, expr_offset + 1, references, match_count);
                        }
                        wz.ascend(bound_copy.len());
                    }
                } else {
                    trace!(
                        name = %self.name,
                        var_ref = k,
                        num_refs = references.len(),
                        "VarRef out of bounds, skipping"
                    );
                }
            }

            Tag::SymbolSize(size) => {
                // Concrete symbol: must match exactly in the trie.
                let symbol_byte = data[expr_offset];
                let symbol_data_start = expr_offset + 1;
                let symbol_data_end = symbol_data_start + size as usize;

                if symbol_data_end > data.len() {
                    return; // Malformed expression
                }

                let symbol_bytes = &data[symbol_data_start..symbol_data_end];

                // Descend: first the tag byte, then the symbol bytes
                wz.descend_to_byte(symbol_byte);
                if wz.path_exists() {
                    wz.descend_to(symbol_bytes);
                    if wz.path_exists() {
                        self.match_item(wz, symbol_data_end, references, match_count);
                    }
                    wz.ascend(size as usize);
                }
                wz.ascend_byte();
            }

            Tag::Arity(arity) => {
                // Concrete arity: must match exactly in the trie.
                let arity_byte = data[expr_offset];
                wz.descend_to_byte(arity_byte);
                if wz.path_exists() {
                    // Recursively match `arity` children
                    self.match_arity_children(wz, expr_offset + 1, arity, references, match_count);
                }
                wz.ascend_byte();
            }
        }
    }

    /// Match `remaining` children of an arity node sequentially.
    ///
    /// Each child is matched by calling `match_item`, which advances the
    /// expression offset past the matched child. After all children are
    /// matched, continues with `match_item` at the post-children offset.
    fn match_arity_children(
        &self,
        wz: &mut WriteZipperTracked<H>,
        expr_offset: usize,
        remaining: u8,
        references: &mut Vec<usize>,
        match_count: &mut usize,
    ) {
        if remaining == 0 {
            // All children matched — continue with whatever follows
            self.match_item(wz, expr_offset, references, match_count);
            return;
        }

        let data = &self.expr_data;
        if expr_offset >= data.len() {
            return;
        }

        // Get the size of the current child item
        let child_size = self.expr_item_size(expr_offset);
        if child_size == 0 {
            return;
        }

        let next_child_offset = expr_offset + child_size;
        let tag = byte_item(data[expr_offset]);

        // Match this child, and for each successful match, continue
        // matching the remaining siblings at next_child_offset.
        match tag {
            Tag::NewVar => {
                let ref_idx = references.len();
                references.push(wz.path().len());

                let cm = wz.child_mask();
                let mut it = cm.iter();
                while let Some(b) = it.next() {
                    let child_tag = byte_item(b);
                    match child_tag {
                        Tag::SymbolSize(size) => {
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                self.enumerate_k_paths_then_siblings(
                                    wz,
                                    size as usize,
                                    next_child_offset,
                                    remaining - 1,
                                    references,
                                    match_count,
                                );
                            }
                            wz.ascend_byte();
                        }
                        Tag::Arity(_) => {
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                self.match_arity_children(
                                    wz,
                                    next_child_offset,
                                    remaining - 1,
                                    references,
                                    match_count,
                                );
                            }
                            wz.ascend_byte();
                        }
                        _ => {
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                self.match_arity_children(
                                    wz,
                                    next_child_offset,
                                    remaining - 1,
                                    references,
                                    match_count,
                                );
                            }
                            wz.ascend_byte();
                        }
                    }
                }

                references.truncate(ref_idx);
            }

            Tag::VarRef(k) => {
                if (k as usize) < references.len() {
                    let ref_path_start = references[k as usize];
                    let current_path = wz.path().to_vec();
                    if ref_path_start < current_path.len() {
                        let bound_copy = current_path[ref_path_start..].to_vec();
                        wz.descend_to(&bound_copy);
                        if wz.path_exists() {
                            self.match_arity_children(
                                wz,
                                next_child_offset,
                                remaining - 1,
                                references,
                                match_count,
                            );
                        }
                        wz.ascend(bound_copy.len());
                    }
                }
            }

            Tag::SymbolSize(size) => {
                let symbol_byte = data[expr_offset];
                let symbol_data_start = expr_offset + 1;
                let symbol_data_end = symbol_data_start + size as usize;
                if symbol_data_end > data.len() {
                    return;
                }
                let symbol_bytes = &data[symbol_data_start..symbol_data_end];

                wz.descend_to_byte(symbol_byte);
                if wz.path_exists() {
                    wz.descend_to(symbol_bytes);
                    if wz.path_exists() {
                        self.match_arity_children(
                            wz,
                            next_child_offset,
                            remaining - 1,
                            references,
                            match_count,
                        );
                    }
                    wz.ascend(size as usize);
                }
                wz.ascend_byte();
            }

            Tag::Arity(sub_arity) => {
                let arity_byte = data[expr_offset];
                wz.descend_to_byte(arity_byte);
                if wz.path_exists() {
                    // Match the sub-arity's children, then continue with
                    // the remaining siblings. We "inline" the sub-arity
                    // children into the remaining count: after matching
                    // sub_arity inner children, we continue with
                    // (remaining - 1) outer siblings.
                    self.match_nested_then_siblings(
                        wz,
                        expr_offset + 1,
                        sub_arity,
                        next_child_offset,
                        remaining - 1,
                        references,
                        match_count,
                    );
                }
                wz.ascend_byte();
            }
        }
    }

    /// Match `inner_remaining` children of a nested arity, then continue
    /// with `outer_remaining` siblings starting at `siblings_offset`.
    fn match_nested_then_siblings(
        &self,
        wz: &mut WriteZipperTracked<H>,
        expr_offset: usize,
        inner_remaining: u8,
        siblings_offset: usize,
        outer_remaining: u8,
        references: &mut Vec<usize>,
        match_count: &mut usize,
    ) {
        if inner_remaining == 0 {
            self.match_arity_children(
                wz,
                siblings_offset,
                outer_remaining,
                references,
                match_count,
            );
            return;
        }

        let data = &self.expr_data;
        if expr_offset >= data.len() {
            return;
        }

        let child_size = self.expr_item_size(expr_offset);
        if child_size == 0 {
            return;
        }

        let next_offset = expr_offset + child_size;
        let tag = byte_item(data[expr_offset]);

        match tag {
            Tag::NewVar => {
                let ref_idx = references.len();
                references.push(wz.path().len());

                let cm = wz.child_mask();
                let mut it = cm.iter();
                while let Some(b) = it.next() {
                    let child_tag = byte_item(b);
                    match child_tag {
                        Tag::SymbolSize(size) => {
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                self.enumerate_k_paths_then_nested(
                                    wz,
                                    size as usize,
                                    next_offset,
                                    inner_remaining - 1,
                                    siblings_offset,
                                    outer_remaining,
                                    references,
                                    match_count,
                                );
                            }
                            wz.ascend_byte();
                        }
                        Tag::Arity(_) | _ => {
                            wz.descend_to_byte(b);
                            if wz.path_exists() {
                                self.match_nested_then_siblings(
                                    wz,
                                    next_offset,
                                    inner_remaining - 1,
                                    siblings_offset,
                                    outer_remaining,
                                    references,
                                    match_count,
                                );
                            }
                            wz.ascend_byte();
                        }
                    }
                }

                references.truncate(ref_idx);
            }

            Tag::VarRef(k) => {
                if (k as usize) < references.len() {
                    let ref_path_start = references[k as usize];
                    let current_path = wz.path().to_vec();
                    if ref_path_start < current_path.len() {
                        let bound_copy = current_path[ref_path_start..].to_vec();
                        wz.descend_to(&bound_copy);
                        if wz.path_exists() {
                            self.match_nested_then_siblings(
                                wz,
                                next_offset,
                                inner_remaining - 1,
                                siblings_offset,
                                outer_remaining,
                                references,
                                match_count,
                            );
                        }
                        wz.ascend(bound_copy.len());
                    }
                }
            }

            Tag::SymbolSize(size) => {
                let symbol_byte = data[expr_offset];
                let start = expr_offset + 1;
                let end = start + size as usize;
                if end > data.len() {
                    return;
                }
                let symbol_bytes = &data[start..end];

                wz.descend_to_byte(symbol_byte);
                if wz.path_exists() {
                    wz.descend_to(symbol_bytes);
                    if wz.path_exists() {
                        self.match_nested_then_siblings(
                            wz,
                            next_offset,
                            inner_remaining - 1,
                            siblings_offset,
                            outer_remaining,
                            references,
                            match_count,
                        );
                    }
                    wz.ascend(size as usize);
                }
                wz.ascend_byte();
            }

            Tag::Arity(sub_arity) => {
                let arity_byte = data[expr_offset];
                wz.descend_to_byte(arity_byte);
                if wz.path_exists() {
                    // Flatten: the sub_arity children are added to the
                    // inner_remaining - 1 count for the current nesting
                    self.match_nested_then_siblings(
                        wz,
                        expr_offset + 1,
                        sub_arity + (inner_remaining - 1),
                        siblings_offset,
                        outer_remaining,
                        references,
                        match_count,
                    );
                }
                wz.ascend_byte();
            }
        }
    }

    /// Enumerate all k-byte paths at the current trie position, and for
    /// each one, continue matching at `continuation_offset`.
    ///
    /// This replaces `descend_first_k_path` / `to_next_k_path` from
    /// `ZipperIteration` which is not available on `WriteZipperTracked`.
    fn enumerate_k_paths(
        &self,
        wz: &mut WriteZipperTracked<H>,
        k: usize,
        continuation_offset: usize,
        references: &mut Vec<usize>,
        match_count: &mut usize,
    ) {
        if k == 0 {
            self.match_item(wz, continuation_offset, references, match_count);
            return;
        }

        let cm = wz.child_mask();
        let mut it = cm.iter();
        while let Some(b) = it.next() {
            wz.descend_to_byte(b);
            if wz.path_exists() {
                self.enumerate_k_paths(wz, k - 1, continuation_offset, references, match_count);
            }
            wz.ascend_byte();
        }
    }

    /// Enumerate all k-byte paths then continue matching arity siblings.
    fn enumerate_k_paths_then_siblings(
        &self,
        wz: &mut WriteZipperTracked<H>,
        k: usize,
        siblings_offset: usize,
        remaining_siblings: u8,
        references: &mut Vec<usize>,
        match_count: &mut usize,
    ) {
        if k == 0 {
            self.match_arity_children(
                wz,
                siblings_offset,
                remaining_siblings,
                references,
                match_count,
            );
            return;
        }

        let cm = wz.child_mask();
        let mut it = cm.iter();
        while let Some(b) = it.next() {
            wz.descend_to_byte(b);
            if wz.path_exists() {
                self.enumerate_k_paths_then_siblings(
                    wz,
                    k - 1,
                    siblings_offset,
                    remaining_siblings,
                    references,
                    match_count,
                );
            }
            wz.ascend_byte();
        }
    }

    /// Enumerate all k-byte paths then continue matching nested arity
    /// children followed by outer siblings.
    fn enumerate_k_paths_then_nested(
        &self,
        wz: &mut WriteZipperTracked<H>,
        k: usize,
        inner_offset: usize,
        inner_remaining: u8,
        siblings_offset: usize,
        outer_remaining: u8,
        references: &mut Vec<usize>,
        match_count: &mut usize,
    ) {
        if k == 0 {
            self.match_nested_then_siblings(
                wz,
                inner_offset,
                inner_remaining,
                siblings_offset,
                outer_remaining,
                references,
                match_count,
            );
            return;
        }

        let cm = wz.child_mask();
        let mut it = cm.iter();
        while let Some(b) = it.next() {
            wz.descend_to_byte(b);
            if wz.path_exists() {
                self.enumerate_k_paths_then_nested(
                    wz,
                    k - 1,
                    inner_offset,
                    inner_remaining,
                    siblings_offset,
                    outer_remaining,
                    references,
                    match_count,
                );
            }
            wz.ascend_byte();
        }
    }

    /// Compute the byte size of the expression item at `offset`.
    ///
    /// Returns the total number of bytes consumed by this item in the
    /// expression encoding:
    /// - `NewVar`: 1 byte
    /// - `VarRef(_)`: 1 byte
    /// - `SymbolSize(n)`: 1 + n bytes
    /// - `Arity(a)`: 1 + sum of sizes of `a` children
    fn expr_item_size(&self, offset: usize) -> usize {
        let data = &self.expr_data;
        if offset >= data.len() {
            return 0;
        }
        match byte_item(data[offset]) {
            Tag::NewVar => 1,
            Tag::VarRef(_) => 1,
            Tag::SymbolSize(n) => 1 + n as usize,
            Tag::Arity(a) => {
                let mut size = 1;
                for _ in 0..a {
                    let child_size = self.expr_item_size(offset + size);
                    if child_size == 0 {
                        return 0;
                    }
                    size += child_size;
                }
                size
            }
        }
    }
}

// Safety: expr_data is Vec<u8> (owned, no raw pointers shared), mode is an enum.
unsafe impl<H: AtomHeader> Send for SExprOperation<H> {}
unsafe impl<H: AtomHeader> Sync for SExprOperation<H> {}

impl<H: AtomHeader + Default> TransformOp<H> for SExprOperation<H> {
    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, wz: &mut WriteZipperTracked<H>) {
        match &self.mode {
            SExprMode::Add => self.apply_add(wz),
            SExprMode::Remove => self.apply_remove(wz),
            SExprMode::Match => self.apply_match(wz),
        }
    }
}
