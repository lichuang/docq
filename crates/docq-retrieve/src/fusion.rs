//! Reciprocal Rank Fusion (RRF) — score-agnostic rank fusion of two recall
//! channels.
//!
//! RRF combines ranked result lists by summing reciprocal ranks: an item at
//! rank `r` (1-indexed) in a channel contributes `1 / (k + r)`. The constant
//! `k` smooths the contribution of top ranks so a single channel cannot
//! dominate. The default `k = 60` follows Cormack, Clarke & Büttcher, SIGIR
//! 2009 ("Reciprocal Rank Fusion outperforms Condorcet and individual rank
//! learning methods").

use std::collections::HashMap;

/// Fuse two ranked result lists into a single ranking.
///
/// Each channel's raw scores are **ignored** — only the rank position
/// matters. This makes RRF robust to the directional mismatch between BM25
/// (higher is better) and cosine distance (lower is better): both are
/// consumed as ordered lists, and the fusion operates purely on positions.
///
/// A chunk that appears in both channels gets two contributions summed
/// together, naturally boosting multi-channel consensus.
pub fn reciprocal_rank_fusion(
  bm25_results: &[(String, f32)],
  vector_results: &[(String, f32)],
  k: usize,
) -> Vec<(String, f32)> {
  let mut scores: HashMap<String, f32> = HashMap::new();

  for (rank, (id, _)) in bm25_results.iter().enumerate() {
    *scores.entry(id.clone()).or_default() += 1.0 / (k + rank + 1) as f32;
  }

  for (rank, (id, _)) in vector_results.iter().enumerate() {
    *scores.entry(id.clone()).or_default() += 1.0 / (k + rank + 1) as f32;
  }

  let mut fused: Vec<(String, f32)> = scores.into_iter().collect();
  fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
  fused
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_rrf_basic() {
    let bm25 = vec![("a".into(), 1.0), ("b".into(), 0.8)];
    let vector = vec![("b".into(), 0.9), ("c".into(), 0.7)];
    let result = reciprocal_rank_fusion(&bm25, &vector, 60);

    let ids: Vec<&str> = result.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids[0], "b");
    assert!(result.len() == 3);
  }

  #[test]
  fn test_rrf_single_channel() {
    let bm25 = vec![("a".into(), 1.0), ("b".into(), 0.8)];
    let result = reciprocal_rank_fusion(&bm25, &[], 60);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0, "a");
  }

  #[test]
  fn test_rrf_empty() {
    let result = reciprocal_rank_fusion(&[], &[], 60);
    assert!(result.is_empty());
  }

  #[test]
  fn test_rrf_score_ordering() {
    let bm25 = vec![("x".into(), 1.0), ("y".into(), 0.5)];
    let vector = vec![("x".into(), 0.9), ("z".into(), 0.3)];
    let result = reciprocal_rank_fusion(&bm25, &vector, 60);
    for w in result.windows(2) {
      assert!(w[0].1 >= w[1].1);
    }
  }
}
