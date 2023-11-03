//! Utilities for working with [Vec] and [std::collections::HashSet].
use std::hash::Hash;

/// Helper function to merge two optional string vectors and dedup any duplicate entries.
pub fn merge_and_dedup_vecs<T: Eq + Hash + Clone + Ord>(
    a: Option<Vec<T>>,
    b: Option<Vec<T>>,
) -> Vec<T> {
    let mut merged = vec![];
    if let Some(a) = a {
        merged.extend(a);
    }
    if let Some(b) = b {
        merged.extend(b);
    }
    merged.sort();
    merged.dedup();
    merged
}
