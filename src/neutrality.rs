//! Per-source bias + factual-tier ratings, mirroring Sacred.Vote's
//! `shared/news-neutrality-helpers.ts` (iter #538 of the main repo).
//!
//! This Rust port preserves the EXACT same formula:
//!   raw = (1 - biasDistance) * BIAS_WEIGHT + factualWeight * FACTUAL_WEIGHT
//!   score = round(raw * 100)
//!
//! Constants pinned at the same values so a future audit can compare
//! TS test outputs to Rust test outputs and get identical results.
//!
//! Used in v0.4+ to annotate each NewsItem before serving from /feeds.
//! This iter (v0.3) ships the helpers + tests; consumer wiring lands
//! in v0.4.
//!
//! Design pins (locked in test, mirroring TS):
//!   - SYMMETRIC-SCORING (left-extreme and right-extreme map to same
//!     |bias_distance| = 1, same neutrality_score for matching factual)
//!   - FORMULA-CONSTANTS-LOCKED (BIAS_WEIGHT=0.6, FACTUAL_WEIGHT=0.4)
//!   - NAN-FOR-AMBIGUOUS-BIAS (returns f64::NAN for "mixed"/"unknown")
//!   - DETERMINISTIC (no randomness, no time reads)
//!   - RENDER-PATH-NO-THROW (formatters never panic)

use serde::{Deserialize, Serialize};

/// Bias rating axis. 9 variants matching TS NEWS_SOURCE_BIAS_RATINGS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BiasRating {
    LeftExtreme,
    Left,
    CenterLeft,
    Center,
    CenterRight,
    Right,
    RightExtreme,
    Mixed,
    Unknown,
}

/// Factual-reporting tier. 5 variants matching TS NEWS_SOURCE_FACTUAL_TIERS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FactualTier {
    VeryHigh,
    High,
    Mixed,
    Low,
    Unknown,
}

/// Composite-score weight split. Pinned policy: 60% bias, 40% factual.
/// MATCHES TS iter #538 BIAS_WEIGHT + FACTUAL_WEIGHT.
pub const BIAS_WEIGHT: f64 = 0.6;
pub const FACTUAL_WEIGHT: f64 = 0.4;

/// Map a bias rating to a numeric score on the [-1, +1] axis.
/// Returns NaN for "mixed" / "unknown" — caller must guard with
/// `f64::is_nan` before treating the result as a magnitude.
pub fn bias_rating_to_score(rating: BiasRating) -> f64 {
    match rating {
        BiasRating::LeftExtreme => -1.0,
        BiasRating::Left => -2.0 / 3.0,
        BiasRating::CenterLeft => -1.0 / 3.0,
        BiasRating::Center => 0.0,
        BiasRating::CenterRight => 1.0 / 3.0,
        BiasRating::Right => 2.0 / 3.0,
        BiasRating::RightExtreme => 1.0,
        BiasRating::Mixed => f64::NAN,
        BiasRating::Unknown => f64::NAN,
    }
}

/// Absolute-value distance from center. NaN for "mixed" / "unknown".
pub fn bias_distance(rating: BiasRating) -> f64 {
    let score = bias_rating_to_score(rating);
    if score.is_nan() {
        f64::NAN
    } else {
        score.abs()
    }
}

/// Factual-tier weight on [0, 1]. Values mirror TS:
///   very-high → 1.0
///   high      → 0.8
///   mixed     → 0.4
///   low       → 0.1
///   unknown   → 0.5  (HALF — neither reward nor punish absence-of-data)
pub fn factual_tier_weight(tier: FactualTier) -> f64 {
    match tier {
        FactualTier::VeryHigh => 1.0,
        FactualTier::High => 0.8,
        FactualTier::Mixed => 0.4,
        FactualTier::Low => 0.1,
        FactualTier::Unknown => 0.5,
    }
}

/// Composite neutrality score on [0, 100]. NaN when bias is "mixed"
/// or "unknown" (no anchor on the axis).
pub fn compute_neutrality_score(bias: BiasRating, factual: FactualTier) -> f64 {
    let dist = bias_distance(bias);
    if dist.is_nan() {
        return f64::NAN;
    }
    let fw = factual_tier_weight(factual);
    let raw = (1.0 - dist) * BIAS_WEIGHT + fw * FACTUAL_WEIGHT;
    (raw * 100.0).round()
}

/// Display label for a bias rating. ASCII, matches TS BIAS_LABELS.
pub fn format_bias_label(rating: BiasRating) -> &'static str {
    match rating {
        BiasRating::LeftExtreme => "Left (Strong)",
        BiasRating::Left => "Left",
        BiasRating::CenterLeft => "Center-Left",
        BiasRating::Center => "Center",
        BiasRating::CenterRight => "Center-Right",
        BiasRating::Right => "Right",
        BiasRating::RightExtreme => "Right (Strong)",
        BiasRating::Mixed => "Mixed",
        BiasRating::Unknown => "Unknown",
    }
}

/// Display label for a factual tier. ASCII, matches TS FACTUAL_LABELS.
pub fn format_factual_label(tier: FactualTier) -> &'static str {
    match tier {
        FactualTier::VeryHigh => "Very High",
        FactualTier::High => "High",
        FactualTier::Mixed => "Mixed",
        FactualTier::Low => "Low",
        FactualTier::Unknown => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_constants_locked() {
        assert_eq!(BIAS_WEIGHT, 0.6);
        assert_eq!(FACTUAL_WEIGHT, 0.4);
        assert_eq!(BIAS_WEIGHT + FACTUAL_WEIGHT, 1.0);
    }

    #[test]
    fn bias_rating_to_score_known_anchors() {
        assert_eq!(bias_rating_to_score(BiasRating::LeftExtreme), -1.0);
        assert_eq!(bias_rating_to_score(BiasRating::Center), 0.0);
        assert_eq!(bias_rating_to_score(BiasRating::RightExtreme), 1.0);
    }

    #[test]
    fn bias_rating_to_score_mid_tiers_thirds() {
        let l = bias_rating_to_score(BiasRating::Left);
        let r = bias_rating_to_score(BiasRating::Right);
        assert!((l - -2.0 / 3.0).abs() < 1e-9);
        assert!((r - 2.0 / 3.0).abs() < 1e-9);
        // Symmetric.
        assert!((l + r).abs() < 1e-9);
    }

    #[test]
    fn bias_rating_to_score_nan_for_mixed_and_unknown() {
        assert!(bias_rating_to_score(BiasRating::Mixed).is_nan());
        assert!(bias_rating_to_score(BiasRating::Unknown).is_nan());
    }

    #[test]
    fn bias_distance_is_abs() {
        for rating in [
            BiasRating::LeftExtreme,
            BiasRating::Left,
            BiasRating::CenterLeft,
            BiasRating::Center,
            BiasRating::CenterRight,
            BiasRating::Right,
            BiasRating::RightExtreme,
        ] {
            let d = bias_distance(rating);
            assert!(d >= 0.0 && d <= 1.0, "rating={:?} dist={}", rating, d);
        }
    }

    #[test]
    fn bias_distance_nan_for_missing() {
        assert!(bias_distance(BiasRating::Mixed).is_nan());
        assert!(bias_distance(BiasRating::Unknown).is_nan());
    }

    #[test]
    fn factual_tier_weight_values_locked() {
        assert_eq!(factual_tier_weight(FactualTier::VeryHigh), 1.0);
        assert_eq!(factual_tier_weight(FactualTier::High), 0.8);
        assert_eq!(factual_tier_weight(FactualTier::Mixed), 0.4);
        assert_eq!(factual_tier_weight(FactualTier::Low), 0.1);
        assert_eq!(factual_tier_weight(FactualTier::Unknown), 0.5);
    }

    #[test]
    fn compute_neutrality_score_center_very_high() {
        // (1 - 0) * 0.6 + 1.0 * 0.4 = 1.0 → 100
        let score = compute_neutrality_score(BiasRating::Center, FactualTier::VeryHigh);
        assert_eq!(score, 100.0);
    }

    #[test]
    fn compute_neutrality_score_extreme_low() {
        // (1 - 1) * 0.6 + 0.1 * 0.4 = 0.04 → 4
        let score = compute_neutrality_score(BiasRating::LeftExtreme, FactualTier::Low);
        assert_eq!(score, 4.0);
    }

    #[test]
    fn compute_neutrality_score_symmetric_left_right() {
        // SYMMETRIC-SCORING: left-extreme and right-extreme should give
        // the same score for the same factual tier.
        for tier in [
            FactualTier::VeryHigh,
            FactualTier::High,
            FactualTier::Mixed,
            FactualTier::Low,
            FactualTier::Unknown,
        ] {
            let l = compute_neutrality_score(BiasRating::LeftExtreme, tier);
            let r = compute_neutrality_score(BiasRating::RightExtreme, tier);
            assert_eq!(l, r, "tier={:?}", tier);
        }
        for tier in [FactualTier::VeryHigh, FactualTier::High] {
            let l = compute_neutrality_score(BiasRating::Left, tier);
            let r = compute_neutrality_score(BiasRating::Right, tier);
            assert_eq!(l, r, "tier={:?}", tier);
        }
    }

    #[test]
    fn compute_neutrality_score_nan_for_mixed_bias() {
        assert!(compute_neutrality_score(BiasRating::Mixed, FactualTier::High).is_nan());
        assert!(compute_neutrality_score(BiasRating::Unknown, FactualTier::High).is_nan());
    }

    #[test]
    fn compute_neutrality_score_monotonic_by_bias() {
        // Higher bias distance → lower score (when factual tier held constant).
        let center = compute_neutrality_score(BiasRating::Center, FactualTier::High);
        let cl = compute_neutrality_score(BiasRating::CenterLeft, FactualTier::High);
        let l = compute_neutrality_score(BiasRating::Left, FactualTier::High);
        let le = compute_neutrality_score(BiasRating::LeftExtreme, FactualTier::High);
        assert!(center > cl);
        assert!(cl > l);
        assert!(l > le);
    }

    #[test]
    fn compute_neutrality_score_monotonic_by_factual() {
        // Higher factual tier weight → higher score (when bias held constant).
        let vh = compute_neutrality_score(BiasRating::Center, FactualTier::VeryHigh);
        let high = compute_neutrality_score(BiasRating::Center, FactualTier::High);
        let mix = compute_neutrality_score(BiasRating::Center, FactualTier::Mixed);
        let low = compute_neutrality_score(BiasRating::Center, FactualTier::Low);
        assert!(vh > high);
        assert!(high > mix);
        assert!(mix > low);
    }

    #[test]
    fn format_bias_label_ascii_only() {
        for rating in [
            BiasRating::LeftExtreme,
            BiasRating::Left,
            BiasRating::CenterLeft,
            BiasRating::Center,
            BiasRating::CenterRight,
            BiasRating::Right,
            BiasRating::RightExtreme,
            BiasRating::Mixed,
            BiasRating::Unknown,
        ] {
            let label = format_bias_label(rating);
            assert!(!label.is_empty());
            assert!(label.is_ascii(), "label={}", label);
        }
    }

    #[test]
    fn format_factual_label_ascii_only() {
        for tier in [
            FactualTier::VeryHigh,
            FactualTier::High,
            FactualTier::Mixed,
            FactualTier::Low,
            FactualTier::Unknown,
        ] {
            let label = format_factual_label(tier);
            assert!(!label.is_empty());
            assert!(label.is_ascii(), "label={}", label);
        }
    }

    #[test]
    fn format_bias_label_specific_strings() {
        // Lock in the exact strings (must match TS).
        assert_eq!(format_bias_label(BiasRating::LeftExtreme), "Left (Strong)");
        assert_eq!(format_bias_label(BiasRating::Center), "Center");
        assert_eq!(
            format_bias_label(BiasRating::RightExtreme),
            "Right (Strong)"
        );
        assert_eq!(format_bias_label(BiasRating::Mixed), "Mixed");
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let a = compute_neutrality_score(BiasRating::Center, FactualTier::High);
        let b = compute_neutrality_score(BiasRating::Center, FactualTier::High);
        assert_eq!(a, b);
    }

    #[test]
    fn neutrality_score_in_normalized_range() {
        // For every defined (bias, factual) where bias has a numeric anchor,
        // score should be in [0, 100].
        for bias in [
            BiasRating::LeftExtreme,
            BiasRating::Left,
            BiasRating::CenterLeft,
            BiasRating::Center,
            BiasRating::CenterRight,
            BiasRating::Right,
            BiasRating::RightExtreme,
        ] {
            for tier in [
                FactualTier::VeryHigh,
                FactualTier::High,
                FactualTier::Mixed,
                FactualTier::Low,
                FactualTier::Unknown,
            ] {
                let score = compute_neutrality_score(bias, tier);
                assert!(!score.is_nan(), "bias={:?} tier={:?}", bias, tier);
                assert!(score >= 0.0, "score below 0: {}", score);
                assert!(score <= 100.0, "score above 100: {}", score);
            }
        }
    }

    #[test]
    fn serde_roundtrip_bias_rating() {
        let r = BiasRating::CenterLeft;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"center-left\"");
        let parsed: BiasRating = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn serde_roundtrip_factual_tier() {
        let t = FactualTier::VeryHigh;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"very-high\"");
        let parsed: FactualTier = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, t);
    }
}
