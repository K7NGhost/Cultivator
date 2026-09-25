//! Turns raw BGE-M3 similarities into "strong" and "related" matches.

use super::store::SemanticSource;
use serde::Serialize;
use std::collections::HashMap;

// Calibrated on the demo case: BGE-M3 cosine scores for short queries are
// compressed (~0.35-0.75), so matches are judged relative to how every item
// scored for the same query. z >= 1.0 kept relevant texts with ~no false
// hits; 0.5-1.0 adds recall with some noise, so it is shown as "related".
const STRONG_Z: f32 = 1.0;
const RELATED_Z: f32 = 0.5;
// z alone always promotes the best of a group, even when nothing in it is
// about the question ("vehicle" against texts topped out at 0.49, "birthday"
// against photos at 0.47, while real hits scored 0.52+). A strong match must
// also clear an absolute score and stand out from the group's mean.
const STRONG_MIN_SCORE: f32 = 0.52;
const STRONG_MIN_MARGIN: f32 = 0.065;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScore {
    pub ref_kind: String,
    pub ref_id: String,
    pub score: f32,
    pub z: f32,
    /// "strong" or "related"; items below the related cutoff are omitted.
    pub tier: &'static str,
}

/// Scores every indexed source against the query (best of its variants),
/// normalizes within each group, and keeps strong and related matches.
pub(super) fn score_sources(
    sources: &[SemanticSource],
    vectors: &HashMap<String, Vec<f32>>,
    query_vectors: &[Vec<f32>],
) -> Vec<SemanticScore> {
    let mut by_group: HashMap<&str, Vec<(&SemanticSource, f32)>> = HashMap::new();

    for source in sources {
        let Some(vector) = vectors.get(&source.hash) else {
            continue;
        };
        let score = query_vectors
            .iter()
            .map(|query| dot(query, vector))
            .fold(f32::MIN, f32::max);
        by_group
            .entry(source.group)
            .or_default()
            .push((source, score));
    }

    let mut scores = Vec::new();
    for entries in by_group.values() {
        let count = entries.len() as f32;
        let mean = entries.iter().map(|(_, score)| score).sum::<f32>() / count;
        let variance = entries
            .iter()
            .map(|(_, score)| (score - mean).powi(2))
            .sum::<f32>()
            / count;
        let std = variance.sqrt();

        for (source, score) in entries {
            let z = if std > 1e-6 {
                (score - mean) / std
            } else {
                0.0
            };
            let is_strong = z >= STRONG_Z
                && *score >= STRONG_MIN_SCORE
                && score - mean >= STRONG_MIN_MARGIN;
            let tier = if is_strong {
                "strong"
            } else if z >= RELATED_Z {
                "related"
            } else {
                continue;
            };
            scores.push(SemanticScore {
                ref_kind: source.ref_kind.to_string(),
                ref_id: source.ref_id.clone(),
                score: *score,
                z,
                tier,
            });
        }
    }

    scores.sort_by(|left, right| right.score.total_cmp(&left.score));
    scores
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(ref_id: &str, group: &'static str, hash: &str) -> SemanticSource {
        SemanticSource {
            ref_kind: "artifact",
            ref_id: ref_id.to_string(),
            group,
            text: String::new(),
            hash: hash.to_string(),
        }
    }

    #[test]
    fn tiers_scores_relative_to_each_group() {
        let sources = vec![
            source("hit", "messages", "a"),
            source("near", "messages", "b"),
            source("miss1", "messages", "c"),
            source("miss2", "messages", "d"),
            source("miss3", "messages", "e"),
            source("unindexed", "messages", "zzz"),
        ];
        let vectors = HashMap::from([
            ("a".to_string(), vec![1.0, 0.0]),
            ("b".to_string(), vec![0.6, 0.8]),
            ("c".to_string(), vec![0.0, 1.0]),
            ("d".to_string(), vec![0.0, 1.0]),
            ("e".to_string(), vec![0.0, 1.0]),
        ]);

        let scores = score_sources(&sources, &vectors, &[vec![1.0, 0.0]]);
        let tiers = scores
            .iter()
            .map(|score| (score.ref_id.as_str(), score.tier))
            .collect::<Vec<_>>();

        assert_eq!(tiers[0], ("hit", "strong"));
        assert!(tiers
            .iter()
            .all(|(id, _)| !id.starts_with("miss") && *id != "unindexed"));
    }

    #[test]
    fn uses_the_best_query_variant() {
        let sources = vec![
            source("x", "messages", "a"),
            source("y", "messages", "b"),
            source("other1", "messages", "c"),
            source("other2", "messages", "c"),
        ];
        let vectors = HashMap::from([
            ("a".to_string(), vec![1.0, 0.0, 0.0]),
            ("b".to_string(), vec![0.0, 1.0, 0.0]),
            ("c".to_string(), vec![0.0, 0.0, 1.0]),
        ]);

        // "x" only matches the first variant and "y" only the second.
        let scores = score_sources(
            &sources,
            &vectors,
            &[vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]],
        );
        let mut matched = scores
            .iter()
            .map(|score| score.ref_id.as_str())
            .collect::<Vec<_>>();
        matched.sort();

        assert_eq!(matched, vec!["x", "y"]);
        assert!(scores
            .iter()
            .all(|score| (score.score - 1.0).abs() < 1e-6 && score.tier == "strong"));
    }

    #[test]
    fn best_of_an_unrelated_group_is_only_related() {
        // Every item scores low for this query; the top one still stands out
        // by z-score, but that alone must not make it a strong match.
        let sources = vec![
            source("best", "media", "a"),
            source("rest1", "media", "b"),
            source("rest2", "media", "b"),
            source("rest3", "media", "b"),
        ];
        let query = vec![1.0, 0.0];
        let vectors = HashMap::from([
            ("a".to_string(), vec![0.47, (1.0_f32 - 0.47 * 0.47).sqrt()]),
            ("b".to_string(), vec![0.43, (1.0_f32 - 0.43 * 0.43).sqrt()]),
        ]);

        let scores = score_sources(&sources, &vectors, &[query]);

        assert_eq!(scores.len(), 1);
        assert_eq!((scores[0].ref_id.as_str(), scores[0].tier), ("best", "related"));
    }
}
