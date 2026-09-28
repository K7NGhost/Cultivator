//! Turns raw BGE-M3 similarities into "strong" and "related" matches, and
//! applies the precision check's verdicts to them.

use super::store::SemanticSource;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

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
// On a large case z-scores alone pass the top few percent of everything
// (5,000+ of 100,000 messages for "money"), so only the best-ranked are kept.
// On a 100,000-message test phone every real hit for "money" and
// "transactions" ranked in the top 25.
const STRONG_LIMIT: usize = 25;
const RELATED_LIMIT: usize = 100;
// The precision check reads each group's best STRONG_LIMIT distinct texts:
// every possible strong match, then the best related ones.
pub(super) const CHECK_LIMIT: usize = STRONG_LIMIT;
// Log-odds of "related" below which the precision check drops a match. With
// Qwen3-Reranker-0.6B every real match on the demo and 100,000-message test
// phones scored -6.0 or higher (indirect ones like "Sent 9,500 to the
// warehouse account." for "money" lowest), while the filler BGE-M3 ranked in
// the top 25 for "money" ("The keys is on the counter.") scored -7.9 or lower.
const CHECK_MIN_LOGIT: f32 = -7.0;

/// A search vector held in memory as 8-bit integers and one scale: a quarter
/// of the float32 size (about 1 KB per item for BGE-M3). Rounding moves a
/// cosine score by ~0.0003, far less than separates real matches.
pub(super) struct QuantizedVector {
    scale: f32,
    values: Vec<i8>,
}

impl QuantizedVector {
    pub fn new(vector: &[f32]) -> Self {
        let largest = vector.iter().fold(0.0_f32, |largest, value| largest.max(value.abs()));
        let scale = if largest > 0.0 { largest / 127.0 } else { 1.0 };
        Self {
            scale,
            values: vector.iter().map(|value| (value / scale).round() as i8).collect(),
        }
    }

    fn dot(&self, query: &[f32]) -> f32 {
        self.scale
            * self
                .values
                .iter()
                .zip(query)
                .map(|(value, query)| f32::from(*value) * query)
                .sum::<f32>()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScore {
    pub ref_kind: String,
    pub ref_id: String,
    pub score: f32,
    pub z: f32,
    /// "strong" or "related"; items below the related cutoff are omitted.
    pub tier: &'static str,
    /// Confirmed by the precision check.
    pub checked: bool,
    #[serde(skip)]
    group: &'static str,
    #[serde(skip)]
    text: String,
}

/// Scores every indexed source against the query (best of its variants),
/// normalizes within each group, and keeps strong and related matches.
pub(super) fn score_sources(
    sources: &[SemanticSource],
    vectors: &HashMap<String, QuantizedVector>,
    query_vectors: &[Vec<f32>],
) -> Vec<SemanticScore> {
    let mut by_group: HashMap<&str, Vec<(&SemanticSource, f32)>> = HashMap::new();

    for source in sources {
        let Some(vector) = vectors.get(&source.hash) else {
            continue;
        };
        let score = query_vectors
            .iter()
            .map(|query| vector.dot(query))
            .fold(f32::MIN, f32::max);
        by_group
            .entry(source.group)
            .or_default()
            .push((source, score));
    }

    let mut scores = Vec::new();
    for entries in by_group.values_mut() {
        let count = entries.len() as f32;
        let mean = entries.iter().map(|(_, score)| score).sum::<f32>() / count;
        let variance = entries
            .iter()
            .map(|(_, score)| (score - mean).powi(2))
            .sum::<f32>()
            / count;
        let std = variance.sqrt();

        // Best first. Only one copy of each distinct text can be a strong
        // answer, so a quick reply sent hundreds of times can't crowd out
        // other hits; its repeats are "related" at most.
        entries.sort_by(|left, right| right.1.total_cmp(&left.1));
        let mut seen_texts = HashSet::new();
        let (mut strong, mut related) = (0, 0);

        for (source, score) in entries.iter() {
            let z = if std > 1e-6 {
                (score - mean) / std
            } else {
                0.0
            };
            let is_strong =
                z >= STRONG_Z && *score >= STRONG_MIN_SCORE && score - mean >= STRONG_MIN_MARGIN;
            // Scores only fall from here, so nothing later qualifies either.
            if (!is_strong && z < RELATED_Z) || (strong >= STRONG_LIMIT && related >= RELATED_LIMIT) {
                break;
            }
            let first_copy = seen_texts.insert(source.hash.as_str());
            let tier = if is_strong && first_copy && strong < STRONG_LIMIT {
                strong += 1;
                "strong"
            } else if related < RELATED_LIMIT {
                related += 1;
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
                checked: false,
                group: source.group,
                text: source.text.clone(),
            });
        }
    }

    scores.sort_by(|left, right| right.score.total_cmp(&left.score));
    scores
}

/// The distinct texts the precision check reads: the best CHECK_LIMIT of
/// each group among `score_sources`' matches (which come best first).
pub(super) fn check_candidates(scores: &[SemanticScore]) -> Vec<String> {
    let mut taken = HashMap::<&str, usize>::new();
    let mut seen = HashSet::new();
    let mut texts = Vec::new();
    for score in scores {
        let count = taken.entry(score.group).or_default();
        if *count < CHECK_LIMIT && seen.insert(score.text.as_str()) {
            *count += 1;
            texts.push(score.text.clone());
        }
    }
    texts
}

/// Keeps the matches the precision check confirmed (`logits` by text) and
/// drops the rest, including any it didn't read.
pub(super) fn apply_check(
    scores: Vec<SemanticScore>,
    logits: &HashMap<String, f32>,
) -> Vec<SemanticScore> {
    scores
        .into_iter()
        .filter(|score| {
            logits
                .get(&score.text)
                .is_some_and(|logit| *logit >= CHECK_MIN_LOGIT)
        })
        .map(|score| SemanticScore {
            checked: true,
            ..score
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quantize(vectors: HashMap<String, Vec<f32>>) -> HashMap<String, QuantizedVector> {
        vectors
            .into_iter()
            .map(|(hash, vector)| (hash, QuantizedVector::new(&vector)))
            .collect()
    }

    #[test]
    fn quantized_vectors_score_like_float32() {
        // Two unit vectors with the spread of real 1,024-dimension embeddings.
        let unit = |seed: u32| {
            let mut state = seed;
            let raw = (0..1024)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 8) as f32 / (1 << 24) as f32 - 0.5
                })
                .collect::<Vec<f32>>();
            let norm = raw.iter().map(|value| value * value).sum::<f32>().sqrt();
            raw.into_iter().map(|value| value / norm).collect::<Vec<f32>>()
        };
        let (document, query) = (unit(1), unit(2));
        let exact = document.iter().zip(&query).map(|(a, b)| a * b).sum::<f32>();
        let quantized = QuantizedVector::new(&document);

        assert!((quantized.dot(&query) - exact).abs() < 0.001);
        assert!((quantized.dot(&document) - 1.0).abs() < 0.001);
        assert_eq!(quantized.values.len(), 1024);
    }

    fn source(ref_id: &str, group: &'static str, hash: &str) -> SemanticSource {
        SemanticSource {
            ref_kind: "artifact",
            ref_id: ref_id.to_string(),
            group,
            text: hash.to_string(),
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

        let scores = score_sources(&sources, &quantize(vectors), &[vec![1.0, 0.0]]);
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
            &quantize(vectors),
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
    fn keeps_only_the_best_ranked_texts_on_a_large_case() {
        // 150 distinct texts that match, plus one best-scoring text sent 50
        // times, among 1,000 that don't.
        let mut sources = (0..150)
            .map(|index| source(&format!("near{index}"), "messages", &format!("near{index}")))
            .collect::<Vec<_>>();
        sources.extend((0..50).map(|index| source(&format!("copy{index}"), "messages", "copy")));
        sources.extend((0..1000).map(|index| source(&format!("far{index}"), "messages", "far")));
        let mut vectors = (0..150)
            .map(|index| (format!("near{index}"), vec![0.9, 0.1]))
            .collect::<HashMap<_, _>>();
        vectors.insert("copy".to_string(), vec![1.0, 0.0]);
        vectors.insert("far".to_string(), vec![0.0, 1.0]);

        let scores = score_sources(&sources, &quantize(vectors), &[vec![1.0, 0.0]]);
        let strong = scores
            .iter()
            .filter(|score| score.tier == "strong")
            .collect::<Vec<_>>();

        // One copy of the repeated text, then 24 distinct texts.
        assert_eq!(strong.len(), STRONG_LIMIT);
        assert_eq!(
            strong.iter().filter(|score| score.ref_id.starts_with("copy")).count(),
            1
        );
        assert_eq!(scores.len(), STRONG_LIMIT + RELATED_LIMIT);
    }

    #[test]
    fn precision_check_drops_rejected_and_unread_matches() {
        // 30 distinct matching texts, more than the check reads, among 200
        // copies of one that doesn't match.
        let mut sources = (0..30)
            .map(|index| source(&format!("m{index}"), "messages", &format!("t{index}")))
            .collect::<Vec<_>>();
        sources.extend((0..200).map(|index| source(&format!("far{index}"), "messages", "far")));
        let mut vectors = (0..30)
            .map(|index| (format!("t{index}"), vec![1.0 - index as f32 * 0.001, 0.1]))
            .collect::<HashMap<_, _>>();
        vectors.insert("far".to_string(), vec![0.0, 1.0]);
        let scores = score_sources(&sources, &quantize(vectors), &[vec![1.0, 0.0]]);

        let candidates = check_candidates(&scores);
        assert_eq!(candidates.len(), CHECK_LIMIT);
        assert_eq!(candidates[0], "t0");

        // The check confirms everything it read except "t1".
        let logits = candidates
            .iter()
            .map(|text| (text.clone(), if text == "t1" { -9.0 } else { -3.0 }))
            .collect();
        let checked = apply_check(scores, &logits);
        let ids = checked
            .iter()
            .map(|score| score.ref_id.as_str())
            .collect::<HashSet<_>>();

        assert!(ids.contains("m0") && !ids.contains("m1"));
        assert!(!ids.contains("m29"), "matches the check didn't read are dropped");
        assert_eq!(checked.len(), CHECK_LIMIT - 1);
        assert!(checked.iter().all(|score| score.checked));
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

        let scores = score_sources(&sources, &quantize(vectors), &[query]);

        assert_eq!(scores.len(), 1);
        assert_eq!((scores[0].ref_id.as_str(), scores[0].tier), ("best", "related"));
    }
}
